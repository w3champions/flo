# Overview

FLO is a Warcraft III toolkit written in Rust:

- Warcraft III server implementation with reconnect, artificial delay, live streaming, and many other features
- Containerized-micro services for easy-to-use server-side API and rapid nodes deployment
- Cross-platform client-side application to run internet games through LAN emulation
- Libraries, for example, W3GS protocol parsing, map file parsing, replay parsing/generating, and LAN game creation

## Setup Local Development Environment

### Install Prerequisites

- C++ Compiler
- CMake
- Latest Stable Rust

### Init submodules

```
git submodule update --init --recursive
```

### Environment Variable Setup Before Cargo Build
BONJOUR_SDK_HOME = <path_to_flo>/deps/bonjour-sdk-windows
PQ_LIB_DIR = <path_to_PostgreSQL>/lib

### Create a .env file:
```
RUST_LOG=debug
DATABASE_URL=postgres://postgres:postgres@localhost/flo
FLO_CONTROLLER_SECRET='1111'
FLO_NODE_SECRET='1111'
JWT_SECRET_BASE64=MTExMQ==
```

### Run migration to create db schema

```
diesel migration run
```

### Add an API client
Insert a row into `api_client` table with secret = `1111` (Corresponds to the values in above .env file)

### Add Players
Insert 2 rows into `player` table with `source_id` = `0` and `api_client_id` equals to the API client id you created in the previous step.
Assuming added player ids are `1` and `2`

### Add Node
Insert a row into `node` with `secret` = `1111` (Corresponds to the values in above .env file)

### Start Node & Controller
```
cargo run -p flo-node-service
cargo run -p flo-controller-service
```

### Start 2 Clients
```
# for player 1
cargo run -p flo-cli -- client 1 connect

# for player 2
cargo run -p flo-cli -- client 2 connect
```

### Create a test game and invite player 1 & 2 to join 

```
cargo run -p flo-cli -- server run-game 1 2
```

### Join the game using LOCAL AREA NETWORK

You will see 2 games correspond to the 2 players

Open 2 Warcraft III to join both games and the game will start.

## Credits

- @nielsAD -- [GoWarcraft3](https://github.com/nielsAD/gowarcraft3)
- Fingon -- Help in game mechanics and algorithms
- @Josko -- [Aura Bot](https://github.com/Josko/aura-bot)
- Varlock -- the author of the GHost++ bot
- @Miezhiko -- initial Linux support
- JSamir/tofik-mamisho -- [wc3-replay-parser](https://github.com/JSamir/wc3-replay-parser)
- PBug90 -- [w3gjs](hhttps://github.com/PBug90/w3gjs)
