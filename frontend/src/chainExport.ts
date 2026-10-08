import { api } from "./api";
import { exportPythonChainStep, parseReplayRequests, pythonChainHeader } from "./requestExport";
import type { ReplayProblem } from "./requestExport";
import type { Chain, Session } from "./types";

export class ChainExportError extends Error {
  constructor(public problem: ReplayProblem | "changed" | "pagination" | "load", public sessionId?: number, public requestIndex?: number, public detail?: string) {
    super(problem);
  }
}

/** Read one metadata page and one payload at a time; retain only generated text. */
export async function loadChainPython(chain: Chain, formatJson: boolean, signal: AbortSignal, progress: (count: number) => void): Promise<string> {
  const chunks = [pythonChainHeader()];
  let cursor: string | undefined;
  let previous: Session | undefined;
  let count = 0;
  do {
    const page = await api.getChainMembers(chain.id, chain.snapshot_id, cursor, signal);
    signal.throwIfAborted();
    if (page.chain.id !== chain.id || page.chain.snapshot_id !== chain.snapshot_id || page.chain.session_count !== chain.session_count) throw new ChainExportError("changed");
    if (page.next_cursor && (page.next_cursor === cursor || !page.items.length)) throw new ChainExportError("pagination");
    for (const member of page.items) {
      if (previous && (member.started_at < previous.started_at || (member.started_at === previous.started_at && member.id <= previous.id))) throw new ChainExportError("pagination");
      let bytes: Uint8Array;
      try { bytes = await api.getPayload(member.id, "c2s", signal); }
      catch (error) { throw new ChainExportError("load", member.id, undefined, error instanceof Error ? error.message : undefined); }
      signal.throwIfAborted();
      const parsed = parseReplayRequests(bytes, formatJson, { host: member.server_ip, port: member.server_port });
      if (!parsed.ok) throw new ChainExportError(parsed.problem, member.id, parsed.requestIndex);
      chunks.push(exportPythonChainStep(member.id, parsed.requests, count === 0));
      previous = member;
      count++;
      if (count > chain.session_count) throw new ChainExportError("changed");
      progress(count);
    }
    cursor = page.next_cursor ?? undefined;
  } while (cursor);
  if (count === 0 || count !== chain.session_count) throw new ChainExportError("changed");
  return chunks.join("");
}
