import type { UploadedFile } from "./helpers"

/**
 * A file attachment extracted from a agent message, tagged with the sender's
 * role. Consumed by the unified {@link AgentAside} rail (Files tab) and the
 * agent-file collector in `helpers.ts`.
 *
 * The old right-rail `FileSidebar` component that lived here was retired in
 * T662 when the Files + Tasks rails were merged into {@link AgentAside}; only
 * this shared shape remains.
 */
export interface AgentFile {
  file: UploadedFile
  role: string
}
