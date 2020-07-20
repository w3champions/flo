# Flo Node

## Symbols

- `->` Incoming packet
- `<-` Outgoing packet
- `<->` Roundtrip

## Protocol

- Listen on port 3551 (0xDDF)

### Controller

#### Connection
- `->` Handshake request with a secret key
- `<->` Ping/Pong between Controller and Node

#### Player Session
- `->` Create Player Session
- `<-` Session Created

#### Game
- `->` Create Game
- `<-` Player Joined
- `<-` Player Left
- `->` Chat Message
- `->` Start Game
- `<-` Game Started
- `<-` Game Ended

### Player

#### Connection
- `->` Handshake request with a session id
- `<-` Player info
- `<->` Ping/Pong between Player and Node

#### Game
- `<-` Join Game Order
- `->` Join Game Ack
- `->` Leave Game Ack
- `->` Chat
- `<-` Slot Updates
- `<-` Start Game
- `->` Command
- `<-` End Game