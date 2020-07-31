pub use crate::proto::flo_connect::*;

packet_type!(ConnectLobby, PacketConnectLobby);
packet_type!(ConnectLobbyAccept, PacketConnectLobbyAccept);
packet_type!(ConnectLobbyReject, PacketConnectLobbyReject);
packet_type!(LobbyDisconnect, PacketLobbyDisconnect);
packet_type!(GameInfo, PacketGameInfo);
packet_type!(GamePlayerEnter, PacketGamePlayerEnter);
packet_type!(GamePlayerLeave, PacketGamePlayerLeave);
packet_type!(GameSlotUpdate, PacketGameSlotUpdate);
packet_type!(GameSlotUpdateRequest, PacketGameSlotUpdateRequest);
packet_type!(PlayerSessionUpdate, PacketPlayerSessionUpdate);
