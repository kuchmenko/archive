import { invoke } from "@tauri-apps/api/core";

export type DocumentKind = "daily" | "note" | "artifact" | "project";
export type DocumentVisibility = "shared" | "private";
export type DocumentAuthor = "user" | "agent";

export type Document = {
  id: number;
  kind: DocumentKind;
  visibility: DocumentVisibility;
  author: DocumentAuthor;
  day: string;
  created_at: string;
  updated_at: string;
  body: string;
  revision: number;
};

export type SyncSnapshot = {
  document: Document | null;
  user_count: number;
  agent_present: boolean;
};

export type ReferenceSummary = {
  id: number;
  kind: DocumentKind;
  day: string;
  label: string;
};

export type AttachmentStatus = "completed" | "blocked" | "failed";

export type AttachmentSummary = {
  artifact_id: number;
  title: string;
  day: string;
  status: AttachmentStatus;
  created_at: string;
  updated_at: string;
  reviewed_at: string | null;
};
export type DailyAttachment = AttachmentSummary;
export type DailyNeighbor = { id: number; day: string };
export type DailyNeighbors = { previous: DailyNeighbor | null; next: DailyNeighbor | null };

export type MermaidDiagnostic = {
  line?: number;
  column?: number;
  message: string;
};

export type MermaidRender = {
  valid: boolean;
  diagram_type?: string;
  diagnostics: MermaidDiagnostic[];
  svg?: string;
};

export function getOrCreateDaily(day: string): Promise<Document> {
  return invoke("get_or_create_daily", { day });
}

export function createNote(day: string, visibility: DocumentVisibility): Promise<Document> {
  return invoke("create_note", { day, visibility });
}
export function createProject(day: string, visibility: DocumentVisibility = "shared"): Promise<Document> {
  return invoke("create_project", { day, visibility });
}
export function addDocumentToProject(projectId: number, documentId: number): Promise<void> {
  return invoke("add_document_to_project", { projectId, documentId });
}
export function listProjectDocuments(projectId: number): Promise<Document[]> {
  return invoke("list_project_documents", { projectId });
}

export function getDocument(id: number): Promise<Document> {
  return invoke("get_document", { id });
}

export function updateDocument(id: number, expectedRevision: number, body: string): Promise<Document> {
  return invoke("update_document_body", { id, expectedRevision, body });
}

export function syncDocument(id: number, knownRevision: number): Promise<SyncSnapshot> {
  return invoke("sync_document", { id, knownRevision });
}

export function updatePresence(sessionId: string, documentId: number): Promise<void> {
  return invoke("update_presence", { sessionId, documentId });
}

export function removePresence(sessionId: string): Promise<void> {
  return invoke("remove_presence", { sessionId });
}

export function deleteNote(id: number): Promise<void> {
  return invoke("delete_note", { id });
}

export function searchDocuments(activeDay: string, query: string): Promise<Document[]> {
  return invoke("search_documents", { activeDay, query });
}

export function resolveReferences(ids: number[]): Promise<ReferenceSummary[]> {
  return invoke("resolve_references", { ids });
}

export function listDailyAttachments(day: string): Promise<DailyAttachment[]> {
  return invoke("list_daily_attachments", { day });
}
export function dailyNeighbors(day: string): Promise<DailyNeighbors> {
  return invoke("daily_neighbors", { day });
}
export function listUnreviewedAttachments(): Promise<AttachmentSummary[]> {
  return invoke("list_unreviewed_attachments");
}
export function getAttachmentByArtifactId(artifactId: number): Promise<AttachmentSummary | null> {
  return invoke("get_attachment_by_artifact_id", { artifactId });
}
export function markAttachmentReviewed(artifactId: number): Promise<AttachmentSummary> {
  return invoke("mark_attachment_reviewed", { artifactId });
}
export function renderMarkdown(markdown: string): Promise<string> {
  return invoke("render_markdown", { markdown });
}

export function renderMermaid(source: string, diagramId: string): Promise<MermaidRender> {
  return invoke("render_mermaid", { source, diagramId });
}
