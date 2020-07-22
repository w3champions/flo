pub mod version;

pub const LOBBY_DOMAIN: &str = "service.w3flo.com";
pub const LOBBY_GRPC_PORT: u16 = 0xDDD;
pub const LOBBY_SOCKET_PORT: u16 = 0xDDE;
pub const CONNECT_WS_PORT: u16 = 0xDDF;
pub const CONNECT_ORIGINS: &[&str] = &["http://localhost:3000", "https://w3flow.com"];
