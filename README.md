# Flo

## Build

### Prerequisites

- C++ Compiler
- CMake
- Latest Stable Rust


```
git submodule update --init --recursive
```

### Windows

```
cargo build --all
```

### Linux/macOS

Only `flo-controller-service` and `flo-node-service` can build on *nix. 
To build release binary for deployment run
```
cargo build -p flo-controller-service --release
cargo build -p flo-node-service --release
```
