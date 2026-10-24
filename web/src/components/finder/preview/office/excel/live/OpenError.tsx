import { EXCEL } from "../excelTheme"

/** Shown inside the Excel frame instead of the grid when the workbook can't open. */
export function OpenError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div
      role="alert"
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 8,
        padding: 24,
        textAlign: "center",
        background: EXCEL.surface,
      }}
    >
      <span style={{ fontSize: 16, fontWeight: 600 }}>Can&apos;t open this workbook</span>
      <span style={{ fontSize: 13, color: EXCEL.inkMuted, maxWidth: 480 }}>{message}</span>
      <button
        type="button"
        onClick={onRetry}
        style={{
          marginTop: 8,
          height: 32,
          padding: "0 16px",
          border: "none",
          borderRadius: 4,
          background: EXCEL.brand,
          color: "#fff",
          font: "inherit",
          fontSize: 14,
          cursor: "pointer",
        }}
      >
        Retry
      </button>
    </div>
  )
}
