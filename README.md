# Overview

FLO is a Warcraft III toolkit written in Rust:

- Warcraft III server implementation with reconnect, artificial delay, live streaming, and many other features
- Containerized-micro services for easy-to-use server-side API and rapid nodes deployment
- Cross-platform client-side application to run internet games through LAN emulation
- Libraries, for example, W3GS protocol parsing, map file parsing, replay parsing/generating, and LAN game creation

## Build

### Prerequisites

- C++ Compiler
- CMake
- Latest Stable Rust

```
git submodule update --init --recursive
cargo build --all
```

## Credits

- @nielsAD -- [GoWarcraft3](https://github.com/nielsAD/gowarcraft3)
- Fingon -- Help in game mechanics and algorithms
- @Josko -- [Aura Bot](https://github.com/Josko/aura-bot)
- Varlock -- the author of the GHost++ bot
- @Miezhiko -- initial Linux support
- JSamir/tofik-mamisho -- [wc3-replay-parser](https://github.com/JSamir/wc3-replay-parser)
- PBug90 -- [w3gjs](hhttps://github.com/PBug90/w3gjs)
