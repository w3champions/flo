pub mod version;

pub const CONTROLLER_DOMAIN: &str = "service.w3flo.com";
pub const CONTROLLER_GRPC_PORT: u16 = 3549;
pub const CONTROLLER_SOCKET_PORT: u16 = 3550;
pub const CLIENT_WS_PORT: u16 = 3551;
pub const CLIENT_ORIGINS: &[&str] = &["http://localhost:3000", "https://w3flo.com"];
pub const NODE_ECHO_PORT: u16 = 3552;
pub const NODE_CONTROLLER_PORT: u16 = 3553;
pub const NODE_CLIENT_PORT: u16 = 3554;
pub const NODE_HTTP_PORT: u16 = 3555;
