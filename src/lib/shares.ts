import { invoke } from "@tauri-apps/api/core";

export type ShareContact = {
  id: string;
  displayName: string;
  syncthingDeviceId?: string | null;
  gatewayDeviceId?: string | null;
  gatewayPublicKey?: string | null;
  inviteState: string;
  lastSeenAt?: number | null;
  createdAt: number;
  updatedAt: number;
};

export type ShareContactInvite = {
  token: string;
  displayName: string;
  syncthingDeviceId: string;
  gatewayDeviceId: string;
  createdAt: number;
};

export type SharedFolderMember = {
  contactId: string;
  addedAt: number;
};

export type SharedWorkspaceFolder = {
  id: string;
  path: string;
  title: string;
  members: SharedFolderMember[];
  direction: string;
  syncthingFolderId?: string | null;
  syncStatus: string;
  createdAt: number;
  updatedAt: number;
};

export type SyncthingShareContactStatus = {
  contactId: string;
  displayName: string;
  deviceId?: string | null;
  inviteState: string;
  status: string;
  configured: boolean;
  connected: boolean;
  address?: string | null;
  connectionType?: string | null;
  lastSeen?: string | null;
  message?: string | null;
};

export type SyncthingSharedFolderStatus = {
  shareId: string;
  path: string;
  title: string;
  status: string;
  state?: string | null;
  configuredMemberCount: number;
  connectedMemberCount: number;
  needItems: number;
  needBytes: number;
  pullErrors: number;
  lastScan?: string | null;
  lastFileAt?: string | null;
  message?: string | null;
};

export type SyncthingIncomingFolderPeer = {
  deviceId: string;
  contactId?: string | null;
  displayName: string;
  offeredAt?: string | null;
  connected: boolean;
};

export type SyncthingIncomingFolderOffer = {
  folderId: string;
  label: string;
  targetPath: string;
  status: string;
  receiveEncrypted: boolean;
  remoteEncrypted: boolean;
  peers: SyncthingIncomingFolderPeer[];
  message?: string | null;
};

export type SyncthingShareStatusSnapshot = {
  running: boolean;
  ready: boolean;
  localDeviceId?: string | null;
  version?: string | null;
  guiUrl: string;
  warning?: string | null;
  error?: string | null;
  contacts: SyncthingShareContactStatus[];
  folders: SyncthingSharedFolderStatus[];
  incomingOffers: SyncthingIncomingFolderOffer[];
};

export type SyncthingFolderConflict = {
  path: string;
};

export type AcceptedSyncthingIncomingFolder = {
  folderId: string;
  label: string;
  path: string;
  snapshot: SyncthingShareStatusSnapshot;
};

export async function listShareContacts(): Promise<ShareContact[]> {
  return invoke<ShareContact[]>("list_share_contacts");
}

export async function saveShareContact(input: {
  contactId?: string;
  displayName: string;
  syncthingDeviceId?: string;
}): Promise<ShareContact> {
  return invoke<ShareContact>("save_share_contact", input);
}

export async function deleteShareContact(contactId: string): Promise<void> {
  await invoke("delete_share_contact", { contactId });
}

export async function listSharedWorkspaceFolders(): Promise<SharedWorkspaceFolder[]> {
  return invoke<SharedWorkspaceFolder[]>("list_shared_workspace_folders");
}

export async function getSharedWorkspaceFolder(
  path: string,
): Promise<SharedWorkspaceFolder | null> {
  return invoke<SharedWorkspaceFolder | null>("get_shared_workspace_folder", { path });
}

export async function saveSharedWorkspaceFolder(input: {
  path: string;
  title?: string;
  memberContactIds: string[];
}): Promise<SharedWorkspaceFolder> {
  return invoke<SharedWorkspaceFolder>("save_shared_workspace_folder", input);
}

export async function deleteSharedWorkspaceFolder(shareId: string): Promise<void> {
  await invoke("delete_shared_workspace_folder", { shareId });
}

export async function generateShareContactInvite(
  displayName?: string,
): Promise<ShareContactInvite> {
  return invoke<ShareContactInvite>("generate_share_contact_invite", { displayName });
}

export async function importShareContactInvite(token: string): Promise<ShareContact> {
  return invoke<ShareContact>("import_share_contact_invite", { token });
}

export async function acceptSyncthingIncomingFolderOffer(
  folderId: string,
): Promise<AcceptedSyncthingIncomingFolder> {
  return invoke<AcceptedSyncthingIncomingFolder>("accept_syncthing_incoming_folder_offer", {
    folderId,
  });
}

export async function rejectSyncthingIncomingFolderOffer(
  folderId: string,
): Promise<SyncthingShareStatusSnapshot> {
  return invoke<SyncthingShareStatusSnapshot>("reject_syncthing_incoming_folder_offer", {
    folderId,
  });
}

export async function getSyncthingShareStatus(): Promise<SyncthingShareStatusSnapshot> {
  return invoke<SyncthingShareStatusSnapshot>("get_syncthing_share_status");
}

export async function syncSyncthingShares(): Promise<SyncthingShareStatusSnapshot> {
  return invoke<SyncthingShareStatusSnapshot>("sync_syncthing_shares");
}

export async function rescanSharedWorkspaceFolder(
  shareId: string,
): Promise<SyncthingShareStatusSnapshot> {
  return invoke<SyncthingShareStatusSnapshot>("rescan_shared_workspace_folder", { shareId });
}

export async function listSharedWorkspaceFolderConflicts(
  shareId: string,
): Promise<SyncthingFolderConflict[]> {
  return invoke<SyncthingFolderConflict[]>("list_shared_workspace_folder_conflicts", { shareId });
}
