# port-alloc

## Installation

Add this package to `Cargo.toml` of your project. (Check https://crates.io/crates/port-alloc for right version)

```toml
[dependencies]
port-alloc = "0.1.0"
```

## Get started

```rust
use port_alloc::PortAlloc;
use std::time::Duration;

let allocator = PortAlloc::new(20000, 65535, Duration::from_seconds(60));
allocator.set_alloc_callback(|id: &[u8]| {
   //
});
allocator.set_dealloc_callback(|id: &[u8]| {
   //
});

let mac_address = "abcdabcdabcd".to_owned();
allocator.alloc_timeout(mac_address, Duration::from_seconds(3));
allocator.wait_process_exit().await?;
```
