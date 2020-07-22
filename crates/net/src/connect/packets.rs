pub use crate::proto::flo_connect::*;

packet_type!(ConnectLobby, PacketConnectLobby);
packet_type!(ConnectLobbyAccept, PacketConnectLobbyAccept);
packet_type!(ConnectLobbyReject, PacketConnectLobbyReject);
packet_type!(LobbyDisconnect, PacketLobbyDisconnect);
