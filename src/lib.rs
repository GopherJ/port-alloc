use fnv::FnvHasher;
use futures_util::stream::StreamExt;
use signal_hook_tokio::v0_3::Signals;

use std::{
    collections::HashMap,
    hash::Hasher,
    io::Error,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    sync::{Arc, RwLock},
    thread,
    time::{Duration, Instant},
};

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hasher = FnvHasher::default();
    hasher.write(bytes);
    hasher.finish()
}

#[derive(Debug)]
pub struct PortEntry {
    pub id: Vec<u8>,
    pub port: u16,
    pub start: Instant,
}

impl PortEntry {
    pub fn new(id: &[u8], port: u16) -> Self {
        Self {
            id: id.to_vec(),
            port,
            start: Instant::now(),
        }
    }
}

struct Inner {
    min: u64,
    max: u64,
    alloc_callback: Option<Box<dyn Fn(&[u8]) + Send + Sync + 'static>>,
    dealloc_callback: Option<Box<dyn Fn(&[u8]) + Send + Sync + 'static>>,
    entries: HashMap<u16, PortEntry>,
    timeout: Duration,
}

impl Inner {
    fn get_port(&self, id: &[u8]) -> u16 {
        (fnv1a(id) % (self.max - self.min) + self.min) as u16
    }

    fn dealloc_timeout_ports(&mut self) {
        let ids: Vec<Vec<u8>> = self
            .entries
            .values()
            .filter(|e| e.start.elapsed() > self.timeout)
            .map(|e| e.id.to_owned())
            .collect();

        for id in ids {
            self.dealloc(&id);
        }
    }

    fn dealloc_all_ports(&mut self) {
        let ids: Vec<Vec<u8>> = self.entries.values().map(|e| e.id.to_owned()).collect();

        for id in ids {
            self.dealloc(&id);
        }
    }

    fn is_open(port: u16) -> bool {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);
        TcpStream::connect(&addr).is_ok()
    }

    fn dealloc(&mut self, id: &[u8]) -> Option<PortEntry> {
        self.entries.remove(&self.get_port(id)).and_then(|e| {
            self.dealloc_callback.as_ref().and_then(|cb| {
                cb(&e.id);
                Some(e)
            })
        })
    }

    fn alloc_timeout(&mut self, id: &[u8], timeout: Duration) -> Option<u16> {
        let port = self.get_port(id);
        self.entries.entry(port).or_insert(PortEntry::new(id, port));
        self.entries
            .get(&port)
            .and_then(|e| if &e.id == id { Some(e.port) } else { None })
            .and_then(|port| {
                if !Inner::is_open(port) {
                    if let Some(ref cb) = self.alloc_callback {
                        cb(id);
                    }

                    let start = Instant::now();
                    loop {
                        if Inner::is_open(port) {
                            return Some(port);
                        } else if start.elapsed() > timeout {
                            self.entries.remove(&port);
                            return None;
                        } else {
                            thread::sleep(Duration::from_millis(250));
                        }
                    }
                } else {
                    return Some(port);
                }
            })
    }
}

#[derive(Clone)]
pub struct PortAlloc {
    inner: Arc<RwLock<Inner>>,
}

impl PortAlloc {
    pub fn new(min: u16, max: u16, timeout: Duration) -> Self {
        let inner = Inner {
            min: min as u64,
            max: max as u64,
            alloc_callback: None,
            dealloc_callback: None,
            entries: HashMap::new(),
            timeout,
        };

        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    pub fn dealloc_timeout_ports(&self) {
        self.inner.write().unwrap().dealloc_timeout_ports()
    }

    pub fn dealloc_all_ports(&self) {
        self.inner.write().unwrap().dealloc_all_ports()
    }

    pub fn set_alloc_callback<F>(&self, f: F)
    where
        F: Fn(&[u8]) + Send + Sync + 'static,
    {
        self.inner.write().unwrap().alloc_callback = Some(Box::new(f));
    }

    pub fn set_dealloc_callback<F>(&self, f: F)
    where
        F: Fn(&[u8]) + Send + Sync + 'static,
    {
        self.inner.write().unwrap().dealloc_callback = Some(Box::new(f));
    }

    pub fn alloc_timeout<T: AsRef<[u8]>>(&self, id: T, timeout: Duration) -> Option<u16> {
        self.inner
            .write()
            .unwrap()
            .alloc_timeout(id.as_ref(), timeout)
    }

    pub fn dealloc<T: AsRef<[u8]>>(&self, id: T) -> Option<PortEntry> {
        self.inner.write().unwrap().dealloc(id.as_ref())
    }

    pub async fn handle_signals(&self, signals: Signals) {
        let mut signals = signals.fuse();
        while let Some(_signal) = signals.next().await {
            self.dealloc_all_ports();
        }
    }

    pub async fn wait_process_exit(&self) -> Result<(), Error> {
        let signals = Signals::new(&[
            signal_hook::SIGHUP,
            signal_hook::SIGTERM,
            signal_hook::SIGINT,
            signal_hook::SIGQUIT,
        ])?;

        let handle = signals.handle();

        let cloned = self.clone();
        let signals_task = tokio::spawn(async move { cloned.handle_signals(signals).await });
        handle.close();
        signals_task.await?;
        Ok(())
    }
}
