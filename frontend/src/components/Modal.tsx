// Overlay + panel from console-object-storage-gate/project/Dashboard.dc.html (lines 200-211).
// Native <dialog> so Escape, focus trapping and inertness come from the platform;
// the prototype's overlay colour lives on dialog::backdrop in styles.css.
import type React from "react";
import { useEffect, useRef, useState } from "react";

function Dialog({
  variant,
  onClose,
  children,
  panel,
}: {
  variant: "confirm" | "form";
  onClose: () => void;
  children: React.ReactNode;
  panel: React.CSSProperties;
}) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    ref.current?.showModal();
  }, []);

  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: Escape is handled by the dialog's own onCancel below; onClick only adds backdrop-click dismissal.
    <dialog
      ref={ref}
      data-variant={variant}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      // A backdrop click targets the dialog itself, never the panel inside it.
      onClick={(e) => {
        if (e.target === ref.current) onClose();
      }}
      style={{
        border: 0,
        padding: 0,
        background: "transparent",
        maxWidth: "100vw",
        maxHeight: "100vh",
      }}
    >
      <div
        style={{
          maxWidth: "calc(100vw - 48px)",
          maxHeight: "90vh",
          overflowY: "auto",
          ...panel,
        }}
      >
        {children}
      </div>
    </dialog>
  );
}

/** Small confirm dialog — Dashboard.dc.html logout modal. */
export function Modal({
  width = 360,
  onClose,
  children,
}: {
  width?: number;
  onClose: () => void;
  children: React.ReactNode;
}) {
  return (
    <Dialog
      variant="confirm"
      onClose={onClose}
      panel={{
        width,
        background: "var(--panel2)",
        border: "1px solid var(--line2)",
        borderRadius: 12,
        padding: 22,
        boxShadow: "0 20px 50px rgba(0,0,0,.55)",
      }}
    >
      {children}
    </Dialog>
  );
}

/**
 * Sectioned form dialog — Buckets.dc.html "Tạo bucket" and its siblings.
 * Compose with FormHead / FormBody / FormFoot.
 */
export function FormModal({
  width = 480,
  danger,
  onClose,
  children,
}: {
  width?: number;
  danger?: boolean;
  onClose: () => void;
  children: React.ReactNode;
}) {
  return (
    <Dialog
      variant="form"
      onClose={onClose}
      panel={{
        width,
        background: "var(--panel)",
        border: `1px solid ${danger ? "rgba(232,82,94,.45)" : "var(--line2)"}`,
        borderRadius: 14,
        boxShadow: "0 30px 70px rgba(0,0,0,.6)",
      }}
    >
      {children}
    </Dialog>
  );
}

export function FormHead({
  title,
  sub,
  danger,
}: {
  title: React.ReactNode;
  sub?: React.ReactNode;
  danger?: boolean;
}) {
  return (
    <div style={{ padding: danger ? "22px 24px 0" : "20px 22px 0" }}>
      <div
        style={{
          fontSize: 16,
          fontWeight: 600,
          color: danger ? "#FF9AA2" : "var(--tx)",
        }}
      >
        {title}
      </div>
      {sub && (
        <div
          style={{
            fontSize: danger ? 13 : 12.5,
            color: "var(--dim)",
            marginTop: danger ? 8 : 5,
            textWrap: "pretty",
          }}
        >
          {sub}
        </div>
      )}
    </div>
  );
}

export function FormBody({
  children,
  padding = "18px 22px",
  gap = 16,
}: {
  children: React.ReactNode;
  padding?: string;
  gap?: number;
}) {
  return (
    <div style={{ padding, display: "flex", flexDirection: "column", gap }}>
      {children}
    </div>
  );
}

export function FormFoot({
  children,
  padding = "14px 22px",
}: {
  children: React.ReactNode;
  padding?: string;
}) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "flex-end",
        gap: 8,
        padding,
        borderTop: "1px solid var(--line)",
      }}
    >
      {children}
    </div>
  );
}

/** Footer "Huỷ" button used by the form modals (dim, not the confirm variant). */
export function FormCancel({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        height: 34,
        padding: "0 14px",
        border: "1px solid var(--line2)",
        background: "var(--panel2)",
        color: "var(--dim)",
        borderRadius: 8,
        fontSize: 13,
        cursor: "pointer",
      }}
    >
      Huỷ
    </button>
  );
}

/** Footer submit button: orange when valid, inert grey otherwise. */
export function FormSubmit({
  label,
  enabled,
  danger,
  onClick,
}: {
  label: string;
  enabled: boolean;
  danger?: boolean;
  onClick: () => void;
}) {
  const bg = enabled ? (danger ? "var(--dgr)" : "var(--acc)") : "var(--panel2)";
  const fg = enabled ? (danger ? "#1A0D03" : "var(--accTx)") : "var(--faint)";
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={!enabled}
      style={{
        height: 34,
        padding: "0 16px",
        border: 0,
        borderRadius: 8,
        background: bg,
        color: fg,
        fontSize: 13,
        fontWeight: 600,
        cursor: enabled ? "pointer" : "not-allowed",
      }}
    >
      {label}
    </button>
  );
}

/**
 * "Type the name to confirm" destructive dialog — spec §5.7 ConfirmDangerDialog.
 * Used for delete bucket, revoke key, delete pool.
 */
export function ConfirmDangerModal({
  title,
  body,
  target,
  confirmLabel,
  onClose,
  onConfirm,
}: {
  title: React.ReactNode;
  body: React.ReactNode;
  target: string;
  confirmLabel: string;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const [typed, setTyped] = useState("");
  const match = typed === target;

  return (
    <FormModal danger onClose={onClose}>
      <FormHead danger title={title} sub={body} />
      <div style={{ padding: "18px 24px" }}>
        <div style={{ fontSize: 12, color: "var(--dim)", marginBottom: 7 }}>
          Gõ{" "}
          <span
            style={{
              fontFamily: "'IBM Plex Mono',monospace",
              color: "var(--tx)",
            }}
          >
            {target}
          </span>{" "}
          để xác nhận
        </div>
        <input
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          style={{
            width: "100%",
            height: 38,
            borderRadius: 8,
            border: "1px solid var(--line2)",
            background: "var(--panel2)",
            color: "var(--tx)",
            padding: "0 12px",
            fontSize: 13.5,
            fontFamily: "'IBM Plex Mono',monospace",
          }}
        />
      </div>
      <FormFoot padding="14px 24px">
        <FormCancel onClick={onClose} />
        <FormSubmit
          danger
          label={confirmLabel}
          enabled={match}
          onClick={() => match && onConfirm()}
        />
      </FormFoot>
    </FormModal>
  );
}

export function ModalTitle({ children }: { children: React.ReactNode }) {
  return (
    <div style={{ fontSize: 15, fontWeight: 600, color: "var(--tx)" }}>
      {children}
    </div>
  );
}

export function ModalText({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontSize: 13,
        color: "var(--dim)",
        marginTop: 8,
        lineHeight: 1.55,
      }}
    >
      {children}
    </div>
  );
}

export function ModalActions({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "flex-end",
        gap: 8,
        marginTop: 20,
      }}
    >
      {children}
    </div>
  );
}

export function CancelButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        height: 34,
        padding: "0 14px",
        border: "1px solid var(--line2)",
        background: "var(--panel)",
        color: "var(--tx)",
        borderRadius: 8,
        fontSize: 13,
        fontWeight: 500,
        cursor: "pointer",
      }}
    >
      Huỷ
    </button>
  );
}

/** Danger action button — enabled only once the typed confirmation matches. */
export function DangerButton({
  label,
  enabled,
  onClick,
}: {
  label: string;
  enabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={!enabled}
      style={{
        height: 34,
        padding: "0 14px",
        border: 0,
        borderRadius: 8,
        background: enabled ? "var(--dgr)" : "var(--panel2)",
        color: enabled ? "#1A0D03" : "var(--faint)",
        fontSize: 13,
        fontWeight: 600,
        cursor: enabled ? "pointer" : "not-allowed",
      }}
    >
      {label}
    </button>
  );
}

export function PrimaryButton({
  label,
  enabled = true,
  onClick,
}: {
  label: string;
  enabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={!enabled}
      style={{
        height: 34,
        padding: "0 14px",
        border: 0,
        borderRadius: 8,
        background: enabled ? "var(--acc)" : "var(--panel2)",
        color: enabled ? "var(--accTx)" : "var(--faint)",
        fontSize: 13,
        fontWeight: 600,
        cursor: enabled ? "pointer" : "not-allowed",
      }}
    >
      {label}
    </button>
  );
}
