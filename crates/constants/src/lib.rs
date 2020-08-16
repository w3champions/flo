pub mod version;

pub const LOBBY_DOMAIN: &str = "service.w3flo.com";
pub const LOBBY_GRPC_PORT: u16 = 3549;
pub const LOBBY_SOCKET_PORT: u16 = 3550;
pub const CONNECT_WS_PORT: u16 = 3551;
pub const CONNECT_ORIGINS: &[&str] = &["http://localhost:3000", "https://w3flow.com"];
pub const NODE_ECHO_PORT: u16 = 3552;
pub const NODE_CONTROLLER_PORT: u16 = 3553;
pub const NODE_CLIENT_PORT: u16 = 3554;
