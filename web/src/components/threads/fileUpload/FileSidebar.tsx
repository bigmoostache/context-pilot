import type { UploadedFile } from "./helpers"

/**
 * A file attachment extracted from a thread message, tagged with the sender's
 * role. Consumed by the unified {@link ThreadAside} rail (Files tab) and the
 * thread-file collector in `helpers.ts`.
 *
 * The old right-rail `FileSidebar` component that lived here was retired in
 * T662 when the Files + Tasks rails were merged into {@link ThreadAside}; only
 * this shared shape remains.
 */
export interface ThreadFile {
  file: UploadedFile
  role: string
}
