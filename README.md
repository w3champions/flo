# Overview

FLO is a Warcraft III server implementation written in Rust.

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

## Credits

- @nielsAD -- [GoWarcraft3](https://github.com/nielsAD/gowarcraft3)
- @Josko -- [Aura Bot](https://github.com/Josko/aura-bot)
- Varlock -- the author of the GHost++ bot