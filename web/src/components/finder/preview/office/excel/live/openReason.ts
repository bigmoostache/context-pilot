/** Why the workbook can't open, or `null` while it can (or is still loading). */
export function openErrorOf(
  liveError: boolean,
  base: Uint8Array | undefined,
  engineError: string | null,
): string | null {
  if (liveError) return "The file could not be loaded from the server."
  if (base && !isZip(base)) return "The file is not an Excel workbook (no .xlsx container)."
  return engineError
}

/** A .xlsx is a zip: it starts with the local-file-header magic "PK\x03\x04". */
const ZIP_MAGIC = [80, 75, 3, 4]

function isZip(bytes: Uint8Array) {
  return ZIP_MAGIC.every((b, i) => bytes[i] === b)
}
