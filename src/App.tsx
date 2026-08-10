import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command";
import {
  MarkdownEditor,
  type ExplorerOrigin,
  type MarkdownEditorHandle,
} from "@/components/MarkdownEditor";
import { MarkdownReader } from "@/components/MarkdownReader";
import {
  createNote,
  createProject,
  addDocumentToProject,
  deleteNote,
  getDocument,
  getOrCreateDaily,
  listDailyAttachments,
  dailyNeighbors,
  getAttachmentByArtifactId,
  listUnreviewedAttachments,
  markAttachmentReviewed,
  listProjectDocuments,
  removePresence,
  resolveReferences,
  searchDocuments,
  syncDocument,
  updateDocument,
  updatePresence,
  type DailyAttachment,
  type DailyNeighbors,
  type Document,
  type ReferenceSummary,
} from "@/lib/archive";
import { AutosaveController, mergeDocumentBodies, type SaveState } from "@/lib/autosave";
import { formatLocalDay, millisecondsUntilNextLocalDay, toLocalDay } from "@/lib/date";
import { documentLabel, formatNoteReference } from "@/lib/documents";
import { addBuffer, adjacentBufferId, removeBuffer, type DocumentBuffer } from "@/lib/buffers";
import { parseNoteReferences } from "@/lib/documents";
import { appShortcut } from "@/lib/shortcuts";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChevronLeft, ChevronRight, FilePlus2, Settings, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function formatAttachmentTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

function statusLabel(status: DailyAttachment["status"]): string {
  switch (status) {
    case "blocked":
      return "Blocked";
    case "failed":
      return "Failed";
    default:
      return "Completed";
  }
}

type DocumentConflict = {
  documentId: number;
  baseBody: string;
  remote: Document;
};

type Presence = {
  userCount: number;
  agentPresent: boolean;
};

export function App() {
  const [today, setToday] = useState(() => toLocalDay(new Date()));
  const [active, setActive] = useState<Document | null>(null);
  const [buffers, setBuffers] = useState<DocumentBuffer[]>([]);
  const [references, setReferences] = useState<ReferenceSummary[]>([]);
  const [vimMode, setVimMode] = useState("NORMAL");
  const [saveState, setSaveState] = useState<SaveState>({ kind: "saved" });
  const [commandsOpen, setCommandsOpen] = useState(false);
  const [explorerOpen, setExplorerOpen] = useState(false);
  const [explorerQuery, setExplorerQuery] = useState("");
  const [explorerResults, setExplorerResults] = useState<Document[]>([]);
  const [explorerSelectedId, setExplorerSelectedId] = useState<number | null>(null);
  const [explorerOrigin, setExplorerOrigin] = useState<ExplorerOrigin | null>(null);
  const [explorerPurpose, setExplorerPurpose] = useState<"open" | "add-to-project">("open");
  const [deleteTargetId, setDeleteTargetId] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);
  const [conflict, setConflict] = useState<DocumentConflict | null>(null);
  const [resolvingConflict, setResolvingConflict] = useState(false);
  const [presence, setPresence] = useState<Presence>({ userCount: 1, agentPresent: false });
  const [attachments, setAttachments] = useState<DailyAttachment[]>([]);
  const [projectDocuments, setProjectDocuments] = useState<Document[]>([]);
  const [shelfExpanded, setShelfExpanded] = useState(false);
  const [mode, setMode] = useState<"edit" | "read">("edit");
  const [neighbors, setNeighbors] = useState<DailyNeighbors>({ previous: null, next: null });
  const [activeAttachment, setActiveAttachment] = useState<DailyAttachment | null>(null);
  const [reviewOpen, setReviewOpen] = useState(false);
  const [reviewQueue, setReviewQueue] = useState<DailyAttachment[]>([]);
  const [reviewLoading, setReviewLoading] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const activeRef = useRef(active);
  const todayRef = useRef(today);
  const operation = useRef(true);
  const editorHandles = useRef(new Map<number, MarkdownEditorHandle>());
  const editorModes = useRef(new Map<number, string>());
  const searchToken = useRef(0);
  const referenceToken = useRef(0);
  const attachmentToken = useRef(0);
  const projectToken = useRef(0);
  const explorerGeneration = useRef(0);
  const addGeneration = useRef<number | null>(null);
  const buffersRef = useRef(buffers);
  const conflictRef = useRef(conflict);
  const explorerOpenRef = useRef(explorerOpen);
  const explorerPurposeRef = useRef(explorerPurpose);
  const syncGeneration = useRef(0);
  const neighborToken = useRef(0);
  const metadataToken = useRef(0);
  const reviewToken = useRef(0);
  const modeToken = useRef(0);
  const todayToken = useRef(0);
  const markReviewToken = useRef(0);
  const reviewPending = useRef(false);
  const sessionId = useRef(
    `archive-gui-${globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`}`,
  ).current;
  activeRef.current = active;
  buffersRef.current = buffers;
  conflictRef.current = conflict;
  explorerOpenRef.current = explorerOpen;
  explorerPurposeRef.current = explorerPurpose;

  const autosaveRef = useRef<AutosaveController | null>(null);
  if (!autosaveRef.current) {
    autosaveRef.current = new AutosaveController(
      async (id, expectedRevision, body) => {
        const updated = await updateDocument(id, expectedRevision, body);
        return updated;
      },
      (state) => {
        setSaveState(state);
        const id = activeRef.current?.id;
        if (id !== undefined) {
          setBuffers((current) => current.map((buffer) =>
            buffer.document.id === id ? { ...buffer, saveState: state } : buffer,
          ));
        }
      },
      650,
      (id, persisted) => {
        setActive((current) =>
          current?.id === id
            ? {
                ...current,
                revision: persisted.revision,
                updated_at: persisted.updated_at ?? current.updated_at,
              }
            : current,
        );
        setBuffers((current) => current.map((buffer) =>
          buffer.document.id === id
            ? {
                ...buffer,
                document: {
                  ...buffer.document,
                  revision: persisted.revision,
                  updated_at: persisted.updated_at ?? buffer.document.updated_at,
                },
              }
            : buffer,
        ));
      },
    );
  }
  const autosave = autosaveRef.current;
  const editorHandle = (documentId: number) => editorHandles.current.get(documentId);
  const activeReferenceIds = active
    ? [...new Set(parseNoteReferences(active.body).map((reference) => reference.id))]
    : [];
  const activeReferenceIdsKey = activeReferenceIds.join(",");

  const showDocument = useCallback(
    (document: Document) => {
      explorerGeneration.current += 1;
      addGeneration.current = null;
      referenceToken.current += 1;
      neighborToken.current += 1;
      metadataToken.current += 1;
      attachmentToken.current += 1;
      setReferences([]);
      setNeighbors({ previous: null, next: null });
      setActiveAttachment(null);
      setAttachments([]);
      setShelfExpanded(false);
      editorHandle(document.id)?.replaceBody(document.body);
      activeRef.current = document;
      setBuffers((current) => current.some((buffer) => buffer.document.id === document.id)
        ? current.map((buffer) => buffer.document.id === document.id ? { ...buffer, document } : buffer)
        : addBuffer(current, document));
      setActive(document);
      modeToken.current += 1;
      todayToken.current += 1;
      markReviewToken.current += 1;
      reviewPending.current = false;
      setMode(document.author === "agent" || document.kind === "artifact" ? "read" : "edit");
      setReviewing(false);
      setVimMode(document.author === "user" && document.kind !== "artifact"
        ? (editorModes.current.get(document.id) ?? "NORMAL")
        : "READ");
      setConflict(null);
      setPresence({ userCount: 1, agentPresent: false });
      autosave.load(document.id, document.body, document.revision);
    },
    [autosave],
  );

  useEffect(() => {
    if (!active) return;
    const token = ++referenceToken.current;
    setReferences([]);
    if (activeReferenceIds.length === 0) return;
    void resolveReferences(activeReferenceIds)
      .then((resolved) => {
        if (token === referenceToken.current && activeRef.current?.id === active.id) setReferences(resolved);
      })
      .catch((error) => {
        if (token === referenceToken.current) setNotice(`References: ${message(error)}`);
      });
  }, [active?.id, activeReferenceIdsKey]);

  useEffect(() => {
    const token = ++neighborToken.current;
    setNeighbors({ previous: null, next: null });
    if (active?.kind !== "daily") return;
    void dailyNeighbors(active.day).then((value) => {
      if (token === neighborToken.current && activeRef.current?.id === active.id) setNeighbors(value);
    }).catch((error) => {
      if (token === neighborToken.current && activeRef.current?.id === active.id) setNotice(`Journal: ${message(error)}`);
    });
  }, [active?.id, active?.kind, active?.day]);

  useEffect(() => {
    const token = ++metadataToken.current;
    setActiveAttachment(null);
    if (!active || (active.kind !== "artifact" && active.author !== "agent")) return;
    void getAttachmentByArtifactId(active.id).then((value) => {
      if (token === metadataToken.current && activeRef.current?.id === active.id) setActiveAttachment(value);
    }).catch((error) => {
      if (token === metadataToken.current && activeRef.current?.id === active.id) setNotice(`Agent work: ${message(error)}`);
    });
  }, [active?.id, active?.kind, active?.author]);

  useEffect(() => {
    if (!reviewOpen) {
      reviewToken.current += 1;
      setReviewLoading(false);
      return;
    }
    const token = ++reviewToken.current;
    setReviewQueue([]);
    setReviewLoading(true);
    void listUnreviewedAttachments().then((rows) => {
      if (token === reviewToken.current) setReviewQueue(rows);
    }).catch((error) => {
      if (token === reviewToken.current) setNotice(`Review: ${message(error)}`);
    }).finally(() => {
      if (token === reviewToken.current) setReviewLoading(false);
    });
  }, [reviewOpen]);

  useEffect(() => {
    if (!active || active.kind !== "daily") {
      setAttachments([]);
      setShelfExpanded(false);
      return;
    }
    const day = active.day;
    const token = ++attachmentToken.current;
    const load = () => {
      void listDailyAttachments(day)
        .then((rows) => {
          if (token !== attachmentToken.current) return;
          if (activeRef.current?.kind !== "daily" || activeRef.current.day !== day) return;
          setAttachments(rows);
        })
        .catch((error) => {
          if (token === attachmentToken.current && activeRef.current?.kind === "daily" && activeRef.current.day === day) {
            setNotice(`Agent work: ${message(error)}`);
          }
        });
    };
    load();
    const timer = setInterval(load, 2_000);
    return () => {
      clearInterval(timer);
    };
  }, [active?.id, active?.kind, active?.day]);

  useEffect(() => {
    const token = ++projectToken.current;
    if (!active || active.kind !== "project") {
      setProjectDocuments([]);
      return;
    }
    const projectId = active.id;
    let latestRequest = 0;
    const load = () => {
      const request = ++latestRequest;
      void listProjectDocuments(projectId).then((rows) => {
        if (token !== projectToken.current || request !== latestRequest) return;
        if (activeRef.current?.id === projectId) setProjectDocuments(rows);
      }).catch((error) => {
        if (token === projectToken.current && request === latestRequest) {
          setNotice(`Project documents: ${message(error)}`);
        }
      });
    };
    load();
    const timer = setInterval(load, 2_000);
    return () => {
      projectToken.current += 1;
      clearInterval(timer);
    };
  }, [active?.id, active?.kind]);

  useEffect(() => {
    let cancelled = false;
    const day = todayRef.current;
    void getOrCreateDaily(day)
      .then((document) => {
        if (!cancelled) showDocument(document);
      })
      .catch((error) => {
        if (!cancelled) setNotice(message(error));
      })
      .finally(() => {
        if (cancelled) return;
        operation.current = false;
        setBusy(false);
        setLoading(false);
      });
    return () => {
      cancelled = true;
      autosave.dispose();
    };
  }, [autosave, showDocument]);

  useEffect(() => {
    return () => {
      void removePresence(sessionId).catch(() => undefined);
    };
  }, [sessionId]);

  useEffect(() => {
    const documentId = active?.id;
    if (documentId === undefined) return;
    const generation = ++syncGeneration.current;
    let disposed = false;
    let polling = false;

    const current = () =>
      !disposed && generation === syncGeneration.current && activeRef.current?.id === documentId;
    const applyDocument = (document: Document) => {
      if (!current()) return;
      editorHandle(documentId)?.replaceBody(document.body);
      activeRef.current = document;
      setActive(document);
      setBuffers((buffers) => buffers.map((buffer) =>
        buffer.document.id === document.id ? { ...buffer, document } : buffer,
      ));
    };
    const saveMerged = (body: string, remote: Document) => {
      const merged = { ...remote, body };
      autosave.resume();
      autosave.rebase(body, remote);
      applyDocument(merged);
      void autosave.flush().catch((error) => {
        if (current()) setNotice(`Could not save merged note: ${message(error)}`);
      });
    };
    const receiveRemote = (remote: Document) => {
      const state = autosave.snapshot();
      if (state.id !== documentId || remote.revision <= state.revision) return;
      const existingConflict = conflictRef.current;
      if (existingConflict?.documentId === documentId) {
        const merged = mergeDocumentBodies(
          existingConflict.baseBody,
          state.body,
          remote.body,
        );
        if (merged.kind === "merged") {
          setConflict(null);
          saveMerged(merged.body, remote);
        } else {
          setConflict({ ...existingConflict, remote });
        }
        return;
      }
      if (!state.dirty) {
        autosave.adoptRemote(remote.body, remote.revision);
        applyDocument(remote);
        return;
      }
      const merged = mergeDocumentBodies(state.persistedBody, state.body, remote.body);
      if (merged.kind === "merged") {
        saveMerged(merged.body, remote);
      } else {
        autosave.pause();
        setConflict({
          documentId,
          baseBody: state.persistedBody,
          remote,
        });
      }
    };
    const poll = async () => {
      if (!current() || polling) return;
      polling = true;
      try {
        const knownRevision = conflictRef.current?.documentId === documentId
          ? conflictRef.current.remote.revision
          : autosave.snapshot().revision;
        const snapshot = await syncDocument(documentId, knownRevision);
        if (!current()) return;
        setPresence({ userCount: snapshot.user_count, agentPresent: snapshot.agent_present });
        if (snapshot.document) receiveRemote(snapshot.document);
      } catch (error) {
        if (current()) setNotice(`Sync: ${message(error)}`);
      } finally {
        polling = false;
      }
    };
    const heartbeat = () => {
      void updatePresence(sessionId, documentId).catch((error) => {
        if (current()) setNotice(`Presence: ${message(error)}`);
      });
    };

    heartbeat();
    void poll();
    const pollTimer = setInterval(() => void poll(), 400);
    const heartbeatTimer = setInterval(heartbeat, 2_500);
    return () => {
      disposed = true;
      clearInterval(pollTimer);
      clearInterval(heartbeatTimer);
    };
  }, [active?.id, autosave, sessionId]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const window = getCurrentWindow();
    void window
      .onCloseRequested(async (event) => {
        event.preventDefault();
        if (await flush()) {
          try {
            await removePresence(sessionId).catch(() => undefined);
            await window.destroy();
          } catch (error) {
            setNotice(`Could not close Archive: ${message(error)}`);
          }
        }
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((error) => setNotice(`Could not monitor window close: ${message(error)}`));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [autosave, sessionId]);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;

    const rollover = async () => {
      if (cancelled) return;
      if (operation.current) {
        timer = setTimeout(() => void rollover(), 1_000);
        return;
      }
      const nextToday = toLocalDay(new Date());
      const previousToday = todayRef.current;
      if (nextToday !== previousToday) {
        if (activeRef.current?.kind === "daily" && activeRef.current.day === previousToday) {
          operation.current = true;
          setBusy(true);
          let switched = false;
          if (await flush()) {
            try {
              const daily = await getOrCreateDaily(nextToday);
              if (await flush()) {
                todayRef.current = nextToday;
                setToday(nextToday);
                showDocument(daily);
                setNotice(null);
                switched = true;
              }
            } catch (error) {
              setNotice(message(error));
            }
          }
          operation.current = false;
          setBusy(false);
          if (!switched) {
            timer = setTimeout(() => void rollover(), 5_000);
            return;
          }
        } else {
          todayRef.current = nextToday;
          setToday(nextToday);
        }
      }
      scheduleMidnight();
    };
    const scheduleMidnight = () => {
      timer = setTimeout(() => void rollover(), millisecondsUntilNextLocalDay(new Date()) + 50);
    };

    scheduleMidnight();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [autosave, showDocument]);

  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.target instanceof Element && event.target.closest(".cm-editor")) return;
      const shortcut = appShortcut(event);
      if (shortcut === "new-note" && !operation.current) {
        event.preventDefault();
        void newStandaloneNote("shared");
      } else if (shortcut === "new-private-note" && !operation.current) {
        event.preventDefault();
        void newStandaloneNote("private");
      } else if (shortcut === "open-commands") {
        event.preventDefault();
        setCommandsOpen(true);
      }
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  });

  useEffect(() => {
    if (!explorerOpen || !active) return;
    const token = ++searchToken.current;
    const timer = setTimeout(() => {
      void searchDocuments(active.day, explorerQuery)
        .then((documents) => {
          if (token !== searchToken.current) return;
          const results = explorerPurpose === "add-to-project" ? documents.filter((document) => document.kind !== "project") : documents;
          setExplorerResults(results);
          setExplorerSelectedId((current) =>
            current !== null && results.some((document) => document.id === current)
              ? current
              : (results[0]?.id ?? null),
          );
        })
        .catch((error) => {
          if (token === searchToken.current) setNotice(`Explorer: ${message(error)}`);
        });
    }, 100);
    return () => clearTimeout(timer);
  }, [active, explorerOpen, explorerQuery, explorerPurpose]);

  async function flush(): Promise<boolean> {
    const id = activeRef.current?.id;
    const snapshot = id === undefined ? null : editorHandle(id)?.snapshot();
    if (snapshot && id !== undefined) {
      setBuffers((current) => current.map((buffer) =>
        buffer.document.id === id ? { ...buffer, editor: snapshot } : buffer,
      ));
    }
    try {
      await autosave.flush();
      return true;
    } catch (error) {
      setNotice(`Could not save note: ${message(error)}`);
      return false;
    }
  }

  async function openDocument(id: number) {
    if (activeRef.current?.id === id || operation.current) return;
    operation.current = true;
    setBusy(true);
    setNotice(null);
    if (await flush()) {
      try {
        const buffered = buffersRef.current.find((buffer) => buffer.document.id === id);
        const document = buffered?.document ?? await getDocument(id);
        if (await flush()) showDocument(document);
      } catch (error) {
        setNotice(`Could not open note: ${message(error)}`);
      }
    }
    operation.current = false;
    setBusy(false);
  }

  async function setReading(next: "edit" | "read") {
    if (next === mode || !active || active.author !== "user" || active.kind === "artifact") return;
    const id = active.id;
    const token = ++modeToken.current;
    if (next === "read" && !(await flush())) return;
    if (token !== modeToken.current || activeRef.current?.id !== id) return;
    setVimMode(next === "read" ? "READ" : (editorModes.current.get(id) ?? "NORMAL"));
    setMode(next);
  }

  async function openToday() {
    if (operation.current || activeRef.current?.kind !== "daily") return;
    const sourceId = activeRef.current.id;
    const token = ++todayToken.current;
    operation.current = true; setBusy(true);
    if (await flush()) {
      try {
        const daily = await getOrCreateDaily(todayRef.current);
        if (token === todayToken.current && activeRef.current?.id === sourceId && await flush()) {
          if (token === todayToken.current && activeRef.current?.id === sourceId) showDocument(daily);
        }
      }
      catch (error) {
        if (token === todayToken.current && activeRef.current?.id === sourceId) setNotice(message(error));
      }
    }
    operation.current = false; setBusy(false);
  }

  async function markReviewed() {
    const current = activeRef.current;
    if (!current || !activeAttachment || activeAttachment.artifact_id !== current.id || reviewPending.current || activeAttachment.reviewed_at) return;
    reviewPending.current = true;
    const token = ++markReviewToken.current;
    setReviewing(true);
    const id = current.id;
    try {
      const updated = await markAttachmentReviewed(id);
      if (token !== markReviewToken.current || activeRef.current?.id !== id) return;
      setActiveAttachment(updated);
      setAttachments((rows) => rows.map((row) => row.artifact_id === id ? updated : row));
      setReviewQueue((rows) => rows.filter((row) => row.artifact_id !== id));
    } catch (error) {
      if (token === markReviewToken.current && activeRef.current?.id === id) setNotice(`Review: ${message(error)}`);
    } finally {
      if (token === markReviewToken.current && activeRef.current?.id === id) {
        reviewPending.current = false;
        setReviewing(false);
      }
    }
  }

  async function newStandaloneNote(visibility: "shared" | "private") {
    if (operation.current) return;
    operation.current = true;
    setBusy(true);
    setNotice(null);
    if (await flush()) {
      try {
        const note = await createNote(todayRef.current, visibility);
        if (await flush()) showDocument(note);
      } catch (error) {
        setNotice(message(error));
      }
    }
    operation.current = false;
    setBusy(false);
  }
  async function newProject() {
    if (operation.current) return;
    operation.current = true;
    setBusy(true);
    setNotice(null);
    if (await flush()) {
      try {
        const project = await createProject(todayRef.current);
        if (await flush()) {
          showDocument(project);
          setTimeout(() => {
            if (activeRef.current?.id === project.id) editorHandle(project.id)?.focus();
          }, 0);
        }
      }
      catch (error) { setNotice(message(error)); }
    }
    operation.current = false;
    setBusy(false);
  }

  async function permanentlyDelete(targetId: number) {
    const current = activeRef.current;
    if (!current || current.id !== targetId || current.kind === "daily" || operation.current) return;
    operation.current = true;
    setBusy(true);
    setNotice(null);
    if (await flush()) {
      try {
        const removed = removeBuffer(buffersRef.current, targetId);
        const survivor = removed.buffers.find((buffer) => buffer.document.id === removed.nextId);
        const replacement = survivor?.document ?? await getOrCreateDaily(current.day);
        await deleteNote(targetId);
        setBuffers(removed.buffers);
        showDocument(replacement);
      } catch (error) {
        setNotice(message(error));
      }
    }
    operation.current = false;
    setBusy(false);
  }

  function editorChanged(documentId: number, body: string) {
    if (documentId !== activeRef.current?.id) return;
    setActive((current) => (current?.id === documentId ? { ...current, body } : current));
    setBuffers((current) => current.map((buffer) =>
      buffer.document.id === documentId
        ? { ...buffer, document: { ...buffer.document, body } }
        : buffer,
    ));
    autosave.change(body);
  }

  function handleNewNoteShortcut() {
    if (operation.current) return false;
    void newStandaloneNote("shared");
    return true;
  }

  function handleNewPrivateNoteShortcut() {
    if (operation.current) return false;
    void newStandaloneNote("private");
    return true;
  }

  function handleOpenCommandsShortcut() {
    setCommandsOpen(true);
    return true;
  }

  function handleOpenExplorer(origin: ExplorerOrigin) {
    if (operation.current || origin.documentId !== activeRef.current?.id || vimMode !== "NORMAL") {
      return false;
    }
    setExplorerOrigin(origin);
    setExplorerPurpose("open");
    setExplorerQuery("");
    setExplorerResults([]);
    setExplorerSelectedId(null);
    setExplorerOpen(true);
    return true;
  }
  function openAddToProject() {
    const current = activeRef.current;
    if (!current || current.kind !== "project") return;
    explorerGeneration.current += 1;
    addGeneration.current = null;
    setCommandsOpen(false);
    setExplorerPurpose("add-to-project");
    setExplorerOrigin({ documentId: current.id, cursor: 0 });
    setExplorerQuery(""); setExplorerResults([]); setExplorerSelectedId(null); setExplorerOpen(true);
  }

  function handleOpenReference(id: number) {
    if (operation.current) return false;
    void openDocument(id);
    return true;
  }

  function switchBuffer(direction: -1 | 1) {
    const current = activeRef.current;
    if (!current || operation.current || vimMode !== "NORMAL" || commandsOpen || explorerOpen || deleteTargetId !== null) return false;
    const id = adjacentBufferId(buffersRef.current, current.id, direction);
    if (id === null) return false;
    void openDocument(id);
    return true;
  }

  function closeExplorer(restoreFocus: boolean) {
    const generation = ++explorerGeneration.current;
    addGeneration.current = null;
    searchToken.current += 1;
    const origin = explorerOrigin;
    setExplorerOpen(false);
    setExplorerOrigin(null);
    setExplorerResults([]);
    setExplorerSelectedId(null);
    if (restoreFocus && origin && activeRef.current?.id === origin.documentId) {
      setTimeout(() => {
        if (generation === explorerGeneration.current && !explorerOpenRef.current && activeRef.current?.id === origin.documentId) {
          editorHandle(origin.documentId)?.focus(origin.cursor);
        }
      }, 1);
    }
  }

  function openExplorerSelection(id: number) {
    if (explorerPurpose === "add-to-project") {
      const project = activeRef.current;
      if (!project || project.kind !== "project" || addGeneration.current !== null) return;
      const generation = explorerGeneration.current;
      addGeneration.current = generation;
      void addDocumentToProject(project.id, id)
        .then(() => {
          if (generation !== explorerGeneration.current || activeRef.current?.id !== project.id) return null;
          return listProjectDocuments(project.id);
        })
        .then((rows) => {
          if (!rows || generation !== explorerGeneration.current || activeRef.current?.id !== project.id || !explorerOpenRef.current || explorerPurposeRef.current !== "add-to-project") return;
          setProjectDocuments(rows);
          closeExplorer(true);
        })
        .catch((error) => {
          if (generation === explorerGeneration.current) setNotice(message(error));
        })
        .finally(() => {
          if (addGeneration.current === generation) addGeneration.current = null;
        });
      return;
    }
    if (id === activeRef.current?.id) {
      closeExplorer(true);
      return;
    }
    closeExplorer(false);
    queueMicrotask(() => void openDocument(id));
  }

  function insertExplorerReference() {
    if (explorerPurposeRef.current !== "open") return;
    const selected = explorerResults.find((document) => document.id === explorerSelectedId);
    const origin = explorerOrigin;
    if (!selected || !origin || activeRef.current?.id !== origin.documentId) return;
    const reference = formatNoteReference(selected);
    closeExplorer(false);
    queueMicrotask(() => {
      if (activeRef.current?.id === origin.documentId) editorHandle(origin.documentId)?.insertAt(origin.cursor, reference);
    });
  }

  function chooseNewNote() {
    setCommandsOpen(false);
    queueMicrotask(() => void newStandaloneNote("shared"));
  }

  function chooseNewPrivateNote() {
    setCommandsOpen(false);
    queueMicrotask(() => void newStandaloneNote("private"));
  }
  function chooseNewProject() { setCommandsOpen(false); queueMicrotask(() => void newProject()); }

  function chooseDelete() {
    const current = activeRef.current;
    if (!current || current.kind === "daily") return;
    const targetId = current.id;
    setCommandsOpen(false);
    queueMicrotask(() => setDeleteTargetId(targetId));
  }

  async function keepLocalConflict() {
    const currentConflict = conflictRef.current;
    if (!currentConflict || resolvingConflict) return;
    setResolvingConflict(true);
    setNotice(null);
    const localBody = autosave.snapshot().body;
    autosave.resume();
    autosave.rebase(localBody, currentConflict.remote);
    const localDocument = { ...currentConflict.remote, body: localBody };
    editorHandle(currentConflict.documentId)?.replaceBody(localBody);
    activeRef.current = localDocument;
    setActive(localDocument);
    setBuffers((current) => current.map((buffer) =>
      buffer.document.id === localDocument.id ? { ...buffer, document: localDocument } : buffer,
    ));
    try {
      await autosave.flush();
      setConflict(null);
      queueMicrotask(() => {
        if (activeRef.current?.id === currentConflict.documentId) editorHandle(currentConflict.documentId)?.focus();
      });
    } catch (error) {
      autosave.pause();
      setNotice(`Could not keep local version: ${message(error)}`);
    } finally {
      setResolvingConflict(false);
    }
  }

  function useRemoteConflict() {
    const currentConflict = conflictRef.current;
    if (!currentConflict || resolvingConflict) return;
    const remote = currentConflict.remote;
    autosave.resume();
    autosave.adoptRemote(remote.body, remote.revision);
    editorHandle(remote.id)?.replaceBody(remote.body);
    activeRef.current = remote;
    setActive(remote);
    setBuffers((current) => current.map((buffer) =>
      buffer.document.id === remote.id ? { ...buffer, document: remote } : buffer,
    ));
    setConflict(null);
    queueMicrotask(() => {
      if (activeRef.current?.id === remote.id) editorHandle(remote.id)?.focus();
    });
  }

  const selectedExplorerDocument =
    explorerResults.find((document) => document.id === explorerSelectedId) ?? null;
  const transientStatus =
    conflict
      ? "Conflict · choose version"
      : saveState.kind === "saving"
      ? "Saving…"
      : saveState.kind === "error"
        ? `Save error: ${saveState.message}`
        : null;
  const title = active
    ? active.kind === "daily"
      ? formatLocalDay(active.day)
      : documentLabel(active)
    : formatLocalDay(today);
  const activeIndex = active ? buffers.findIndex((buffer) => buffer.document.id === active.id) : -1;
  const attachmentCounts = useMemo(() => ({
    blocked: attachments.filter((item) => item.status === "blocked").length,
    failed: attachments.filter((item) => item.status === "failed").length,
    new: attachments.filter((item) => item.reviewed_at === null).length,
  }), [attachments]);
  const showAgentShelf = active?.kind === "daily" && attachments.length > 0;
  const currentAttachment = activeAttachment?.artifact_id === active?.id ? activeAttachment : null;

  function setReviewDialogOpen(open: boolean) {
    if (!open) {
      reviewToken.current += 1;
      setReviewLoading(false);
    }
    setReviewOpen(open);
  }

  return (
    <div className="grid h-screen grid-rows-[minmax(0,1fr)_28px] overflow-hidden bg-background text-foreground">
      <main className="min-h-0 overflow-y-auto bg-[radial-gradient(circle_at_50%_-15%,#292d37_0,transparent_38%)]">
        <div className="mx-auto w-full max-w-[820px] px-6 pb-32 pt-12">
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="min-w-0 flex-1 text-3xl font-semibold tracking-[-0.035em] text-[#f3efe7]">{title}</h1>
            {active?.kind === "daily" && <>
              <button type="button" className="rounded-md p-2 hover:bg-white/5 disabled:opacity-40" disabled={!neighbors.previous || busy} aria-label={neighbors.previous ? `Previous daily: ${formatLocalDay(neighbors.previous.day)}` : "No previous daily"} onClick={() => neighbors.previous && void openDocument(neighbors.previous.id)}><ChevronLeft className="size-4" /></button>
              <button type="button" className="rounded-md p-2 hover:bg-white/5 disabled:opacity-40" disabled={!neighbors.next || busy} aria-label={neighbors.next ? `Next daily: ${formatLocalDay(neighbors.next.day)}` : "No next daily"} onClick={() => neighbors.next && void openDocument(neighbors.next.id)}><ChevronRight className="size-4" /></button>
              {active.day !== today && <button type="button" className="rounded-md px-3 py-2 text-sm hover:bg-white/5" onClick={() => void openToday()}>Today</button>}
            </>}
            {active?.author === "user" && active.kind !== "artifact" && <button type="button" className="rounded-md px-3 py-2 text-sm text-muted-foreground hover:bg-white/5 hover:text-foreground" onClick={() => void setReading(mode === "edit" ? "read" : "edit")}>{mode === "edit" ? "Read" : "Edit"}</button>}
          </div>

          {notice && (
            <div className="mt-4 rounded-md border border-destructive/25 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              {notice}
            </div>
          )}

          {loading || !active ? (
            <p className="mt-12 text-sm text-muted-foreground">Loading note…</p>
          ) : (
            <section className="mt-10 min-h-40">
              {mode === "read" && <MarkdownReader documentId={active.id} body={active.body} onOpenReference={(id) => void openDocument(id)} />}
              {buffers.filter((buffer) =>
                buffer.document.author === "user" &&
                (buffer.document.kind === "daily" || buffer.document.kind === "note" || buffer.document.kind === "project")
              ).sort((left, right) =>
                Number(right.document.id === active.id) - Number(left.document.id === active.id)
              ).map((buffer) => {
                const editorActive = active.id === buffer.document.id && mode === "edit";
                return <div
                  key={buffer.document.id}
                  data-document-id={buffer.document.id}
                  data-editor-active={editorActive}
                  hidden={!editorActive}
                >
                  <MarkdownEditor
                    ref={(handle) => {
                      if (handle) editorHandles.current.set(buffer.document.id, handle);
                      else editorHandles.current.delete(buffer.document.id);
                    }}
                    entryId={buffer.document.id}
                    body={buffer.document.body}
                    active={editorActive}
                    readOnly={!editorActive || busy || conflict !== null}
                    onChange={editorChanged}
                    onModeChange={(documentId, nextMode) => {
                      editorModes.current.set(documentId, nextMode);
                      if (activeRef.current?.id === documentId && mode === "edit") setVimMode(nextMode);
                    }}
                    onNewNote={handleNewNoteShortcut}
                    onNewPrivateNote={handleNewPrivateNoteShortcut}
                    onOpenCommands={handleOpenCommandsShortcut}
                    onOpenExplorer={handleOpenExplorer}
                    onOpenReference={handleOpenReference}
                    references={editorActive ? references : []}
                    initialSnapshot={buffer.editor}
                    onPreviousBuffer={() => switchBuffer(-1)}
                    onNextBuffer={() => switchBuffer(1)}
                  />
                </div>;
              })}
              {(active.kind === "artifact" || active.author === "agent") && <div className="mt-6 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                <span>Agent</span>
                {currentAttachment && <><span>· {formatLocalDay(currentAttachment.day)}</span><span>· {statusLabel(currentAttachment.status)}</span><span>· {currentAttachment.reviewed_at ? "Reviewed" : "New"}</span>{!currentAttachment.reviewed_at && <button type="button" className="ml-auto rounded-md border border-border px-3 py-2 text-sm text-foreground hover:bg-white/5 disabled:opacity-50" disabled={reviewing} onClick={() => void markReviewed()}>{reviewing ? "Marking…" : "Mark reviewed"}</button>}</>}
              </div>}
              {showAgentShelf && (
                <div className="mt-10 border-t border-border/60 pt-4">
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded-md px-1 py-1.5 text-left text-sm text-muted-foreground transition-colors hover:bg-white/5 hover:text-foreground"
                    onClick={() => setShelfExpanded((value) => !value)}
                    aria-expanded={shelfExpanded}
                  >
                    <ChevronRight
                      className={`size-3.5 shrink-0 transition-transform ${shelfExpanded ? "rotate-90" : ""}`}
                    />
                    <span className="min-w-0 flex-1 truncate">
                      Agent work · {attachments.length}
                    </span>
                    <span className="flex shrink-0 gap-2 text-xs text-amber-200/90">{attachmentCounts.blocked > 0 && <span>{attachmentCounts.blocked} blocked</span>}{attachmentCounts.failed > 0 && <span>{attachmentCounts.failed} failed</span>}{attachmentCounts.new > 0 && <span>{attachmentCounts.new} New</span>}</span>
                  </button>
                  {shelfExpanded && (
                    <ul className="mt-2 space-y-1">
                      {attachments.map((attachment) => (
                        <li key={attachment.artifact_id}>
                          <button
                            type="button"
                            className="flex w-full items-start gap-3 rounded-md px-2 py-2 text-left transition-colors hover:bg-white/5"
                            onClick={() => void openDocument(attachment.artifact_id)}
                            disabled={busy || conflict !== null}
                          >
                            <span className="min-w-0 flex-1">
                              <span className="block truncate text-sm text-[#f3efe7]">
                                {attachment.title}
                              </span>
                              <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                                {formatAttachmentTime(attachment.updated_at)}
                              </span>
                            </span>
                            <span
                              className={`shrink-0 pt-0.5 text-[11px] uppercase tracking-[0.06em] ${
                                attachment.status === "completed"
                                  ? "text-muted-foreground"
                                  : "text-amber-200/90"
                              }`}
                            >
                              {statusLabel(attachment.status)}
                            </span>
                            {attachment.reviewed_at === null && <span className="shrink-0 pt-0.5 text-[11px] uppercase text-foreground">New</span>}
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              )}
              {active.kind === "project" && (
                <div className="mt-10 border-t border-border/60 pt-4">
                  <div className="flex items-center justify-between gap-4">
                    <h2 className="text-sm font-medium text-[#f3efe7]">Project documents</h2>
                    <button type="button" className="rounded-md px-3 py-2 text-sm text-muted-foreground hover:bg-white/5 hover:text-foreground" onClick={openAddToProject}>Add document</button>
                  </div>
                  {projectDocuments.length === 0 ? <p className="mt-3 text-sm text-muted-foreground">Add notes, daily documents, or artifacts to this project.</p> : (
                    <ul className="mt-2 space-y-1">{projectDocuments.map((document) => <li key={document.id}><button type="button" className="flex w-full items-center justify-between gap-3 rounded-md px-2 py-2 text-left hover:bg-white/5" onClick={() => void openDocument(document.id)}><span className="truncate text-sm">{documentLabel(document)}</span><span className="shrink-0 text-xs text-muted-foreground">{document.kind === "daily" ? "Daily" : document.kind === "artifact" ? "Artifact" : "Note"} · {document.day} · {document.author === "agent" ? "Agent" : "You"}</span></button></li>)}</ul>
                  )}
                </div>
              )}
            </section>
          )}
        </div>
      </main>

      <footer
        className="grid grid-cols-[minmax(0,1fr)_minmax(0,auto)_minmax(0,1fr)] items-center border-t border-border/70 bg-[#17191f] px-3 font-mono text-[10px] uppercase tracking-[0.08em] text-muted-foreground select-none"
        data-tauri-drag-region
      >
        <span className="min-w-0 truncate justify-self-start" data-status-region="identity" data-tauri-drag-region>
          Archive <span className="px-1 text-border">·</span> {vimMode}
        </span>
        <span
          className="min-w-0 max-w-full truncate px-3 text-center normal-case"
          data-status-region="document"
          data-tauri-drag-region
        >
          {active && (
            <>
            {activeIndex + 1}/{buffers.length} · {documentLabel(active)} · {mode === "read" ? "Read" : "Edit"}
            {active.visibility === "private" ? " · Private" : ""}
            {active.kind === "artifact" ? " · Artifact" : ""}
            {active.kind === "project" ? " · Project" : ""}
            {active.author === "agent" ? " · Agent" : ""}
            {presence.userCount > 1 ? ` · ${presence.userCount} viewers` : ""}
            {active.visibility === "shared" && presence.agentPresent ? " · ✦ Agent present" : ""}
            </>
          )}
        </span>
        <span
          className={`min-w-0 max-w-full truncate justify-self-end text-right ${saveState.kind === "error" ? "text-destructive" : ""}`}
          data-status-region="persistence"
          data-tauri-drag-region
        >
          {transientStatus}
        </span>
      </footer>

      <CommandDialog
        open={explorerOpen}
        onOpenChange={(open) => {
          if (!open) closeExplorer(true);
        }}
        title={explorerPurpose === "add-to-project" ? "Add document to project" : "Explore notes"}
        description={explorerPurpose === "add-to-project" ? "Search for a document to add" : "Search and open Archive documents"}
        className="sm:max-w-2xl"
      >
        <Command
          shouldFilter={false}
          value={explorerSelectedId === null ? "" : String(explorerSelectedId)}
          onValueChange={(value) => setExplorerSelectedId(value ? Number(value) : null)}
          onKeyDownCapture={(event) => {
            if (event.ctrlKey && event.key === "Enter") {
              event.preventDefault();
              event.stopPropagation();
              if (explorerPurpose === "open") insertExplorerReference();
            }
          }}
        >
          <CommandInput
            value={explorerQuery}
            onValueChange={(value) => {
              searchToken.current += 1;
              setExplorerResults([]);
              setExplorerSelectedId(null);
              setExplorerQuery(value);
            }}
            placeholder={explorerPurpose === "add-to-project" ? "Search documents…" : "Search notes…"}
          />
          <div className="grid min-h-72 grid-cols-[minmax(0,1fr)_minmax(0,1fr)] border-t border-border/60">
            <CommandList className="max-h-80 border-r border-border/60 p-1">
              <CommandEmpty>{explorerPurpose === "add-to-project" ? "No matching documents." : "No matching notes."}</CommandEmpty>
              <CommandGroup heading="Documents">
                {explorerResults.map((document) => (
                  <CommandItem
                    key={document.id}
                    value={String(document.id)}
                    onSelect={() => openExplorerSelection(document.id)}
                    className="items-start"
                  >
                    <span className="min-w-0 flex-1 truncate">{documentLabel(document)}</span>
                    <span className="shrink-0 text-[10px] uppercase text-muted-foreground">
                      {document.kind === "daily" ? "Daily" : document.kind === "artifact" ? "Artifact" : document.kind === "project" ? "Project" : "Note"}
                      {document.visibility === "private" ? " · Private" : ""}
                      {document.author === "agent" ? " · Agent" : ""} · {document.day}
                    </span>
                  </CommandItem>
                ))}
              </CommandGroup>
            </CommandList>
            <div className="max-h-80 overflow-auto p-4">
              {selectedExplorerDocument ? (
                <>
                  <div className="mb-3 text-xs text-muted-foreground">
                    {selectedExplorerDocument.kind === "daily" ? "Daily" : selectedExplorerDocument.kind === "artifact" ? "Artifact" : selectedExplorerDocument.kind === "project" ? "Project" : "Note"}
                    {selectedExplorerDocument.visibility === "private" ? " · Private" : ""}
                    {selectedExplorerDocument.author === "agent" ? " · Agent" : ""} · {selectedExplorerDocument.day}
                  </div>
                  <pre className="whitespace-pre-wrap font-mono text-xs leading-5 text-foreground">
                    {selectedExplorerDocument.body || "Untitled note"}
                  </pre>
                </>
              ) : (
                <span className="text-xs text-muted-foreground">No note selected</span>
              )}
            </div>
          </div>
          <div className="flex justify-end gap-4 border-t border-border/60 px-3 py-2 text-[10px] text-muted-foreground">
            <span>Enter {explorerPurpose === "add-to-project" ? "add" : "open"}</span>
            {explorerPurpose === "open" && <span>Ctrl Enter reference</span>}
            <span>Esc close</span>
          </div>
        </Command>
      </CommandDialog>

      <CommandDialog open={commandsOpen} onOpenChange={setCommandsOpen}>
        <Command>
          <CommandInput placeholder="Type a command…" />
          <CommandList>
            <CommandEmpty>No matching commands.</CommandEmpty>
            <CommandGroup heading="Archive">
              <CommandItem onSelect={chooseNewNote} disabled={busy}>
                <FilePlus2 />
                New note
                <CommandShortcut>Ctrl N</CommandShortcut>
              </CommandItem>
              <CommandItem onSelect={chooseNewPrivateNote} disabled={busy}>
                <FilePlus2 />
                Private note
                <CommandShortcut>Ctrl Shift N</CommandShortcut>
              </CommandItem>
              <CommandItem onSelect={chooseNewProject} disabled={busy}><FilePlus2 />New project</CommandItem>
              {active?.kind === "project" && <CommandItem onSelect={openAddToProject} disabled={busy}><FilePlus2 />Add document</CommandItem>}
              {active?.author === "user" && active.kind !== "artifact" && <CommandItem onSelect={() => { setCommandsOpen(false); queueMicrotask(() => void setReading(mode === "edit" ? "read" : "edit")); }}>{mode === "edit" ? "Read document" : "Edit document"}</CommandItem>}
              <CommandItem onSelect={() => { setCommandsOpen(false); setReviewDialogOpen(true); }}>Review agent work</CommandItem>
              <CommandItem onSelect={chooseDelete} disabled={active?.kind === "daily" || busy}>
                <Trash2 />
                {active?.kind === "daily" ? "Daily documents cannot be deleted" : `Delete ${active?.kind ?? "note"} permanently…`}
              </CommandItem>
              <CommandItem disabled>
                <Settings />
                Settings
                <CommandShortcut>Coming later</CommandShortcut>
              </CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>
      </CommandDialog>

      <CommandDialog open={reviewOpen} onOpenChange={setReviewDialogOpen} title="Review agent work" description="Unreviewed daily-attached agent work">
        <Command>
          <CommandInput placeholder="Filter agent work…" />
          <CommandList>
            {reviewLoading ? (
              <div role="status" aria-busy="true" className="py-6 text-center text-sm text-muted-foreground">
                Loading agent work…
              </div>
            ) : (
              <>
                <CommandEmpty>No agent work is waiting for review.</CommandEmpty>
                <CommandGroup heading="New agent work">
                  {reviewQueue.map((item) => (
                    <CommandItem
                      key={item.artifact_id}
                      value={`${item.title} ${item.day} ${item.status}`}
                      onSelect={() => {
                        setReviewDialogOpen(false);
                        queueMicrotask(() => void openDocument(item.artifact_id));
                      }}
                    >
                      <span className="min-w-0 flex-1 truncate">{item.title}</span>
                      <span className="shrink-0 text-xs text-muted-foreground">
                        {formatLocalDay(item.day)} · {statusLabel(item.status)} · New
                      </span>
                    </CommandItem>
                  ))}
                </CommandGroup>
              </>
            )}
          </CommandList>
        </Command>
      </CommandDialog>

      <AlertDialog
        open={conflict !== null}
        onOpenChange={() => undefined}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Concurrent edits need your choice</AlertDialogTitle>
            <AlertDialogDescription>
              Archive preserved both versions because the same lines changed here and in another process.
              Keep your exact text or replace it with the latest remote version.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={resolvingConflict} onClick={useRemoteConflict}>
              Use remote
            </AlertDialogCancel>
            <AlertDialogAction disabled={resolvingConflict} onClick={() => void keepLocalConflict()}>
              Keep mine
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        open={deleteTargetId !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTargetId(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Permanently delete this {active?.kind === "project" ? "project" : "note"}?</AlertDialogTitle>
            <AlertDialogDescription>
              {active?.kind === "project" ? "This project will be removed immediately. Member documents will be retained." : "This standalone note will be removed immediately. Trash and recovery are not available yet."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                if (deleteTargetId !== null) void permanentlyDelete(deleteTargetId);
                setDeleteTargetId(null);
              }}
            >
              {active?.kind === "project" ? "Delete project" : "Delete permanently"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
