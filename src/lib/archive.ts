import { invoke } from "@tauri-apps/api/core";

export type DocumentKind = "daily" | "note" | "artifact";
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
};

export type ReferenceSummary = {
  id: number;
  kind: DocumentKind;
  day: string;
  label: string;
};

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

export function getDocument(id: number): Promise<Document> {
  return invoke("get_document", { id });
}

export function updateDocument(id: number, expectedBody: string, body: string): Promise<Document> {
  return invoke("update_document_body", { id, expectedBody, body });
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

export function renderMermaid(source: string, diagramId: string): Promise<MermaidRender> {
  return invoke("render_mermaid", { source, diagramId });
}
