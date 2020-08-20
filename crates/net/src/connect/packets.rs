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
packet_type!(ListNodesRequest, PacketListNodesRequest);
packet_type!(ListNodes, PacketListNodes);
packet_type!(GameSelectNodeRequest, PacketGameSelectNodeRequest);
packet_type!(GameSelectNode, PacketGameSelectNode);
packet_type!(
  GamePlayerPingMapUpdateRequest,
  PacketGamePlayerPingMapUpdateRequest
);
packet_type!(GamePlayerPingMapUpdate, PacketGamePlayerPingMapUpdate);
packet_type!(
  GamePlayerPingMapSnapshotRequest,
  PacketGamePlayerPingMapSnapshotRequest
);
packet_type!(GamePlayerPingMapSnapshot, PacketGamePlayerPingMapSnapshot);
packet_type!(GamePlayerToken, PacketGamePlayerToken);
packet_type!(GameStartRequest, PacketGameStartRequest);
packet_type!(GameStarting, PacketGameStarting);
packet_type!(GameStartReject, PacketGameStartReject);
packet_type!(
  GameStartPlayerClientInfoRequest,
  PacketGameStartPlayerClientInfoRequest
);
