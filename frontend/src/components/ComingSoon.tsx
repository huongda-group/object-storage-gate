type Props = { title: string; reason: string };

/**
 * A screen with no backend yet.
 *
 * Deliberately has no controls, not even disabled ones: a greyed-out button still says the
 * feature exists and is merely switched off, and this console previously shipped forms that
 * accepted production credentials and threw them away on reload.
 */
export function ComingSoon({ title, reason }: Props) {
  return (
    <div
      style={{
        maxWidth: 460,
        margin: "64px auto",
        textAlign: "center",
        display: "flex",
        flexDirection: "column",
        gap: 12,
      }}
    >
      <div style={{ fontSize: 20, fontWeight: 600 }}>{title}</div>
      <div style={{ fontSize: 13.5, color: "var(--dim)", lineHeight: 1.6 }}>
        {reason}
      </div>
    </div>
  );
}
