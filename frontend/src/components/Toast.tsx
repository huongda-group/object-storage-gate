// Toast markup from console-object-storage-gate/project/Dashboard.dc.html (lines 213-218).
import type React from "react";
import {
  createContext,
  useCallback,
  useContext,
  useRef,
  useState,
} from "react";

type ToastKind = "ok" | "danger";
type ShowToast = (msg: string, kind?: ToastKind) => void;

const ToastContext = createContext<ShowToast>(() => {});

export function useToast(): ShowToast {
  return useContext(ToastContext);
}

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toast, setToast] = useState<{ msg: string; kind: ToastKind } | null>(
    null,
  );
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const show = useCallback<ShowToast>((msg, kind = "ok") => {
    setToast({ msg, kind });
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setToast(null), 2600);
  }, []);

  const danger = toast?.kind === "danger";

  return (
    <ToastContext.Provider value={show}>
      {children}
      {toast && (
        <div
          style={{
            position: "fixed",
            top: 16,
            right: 20,
            zIndex: 200,
            display: "flex",
            alignItems: "center",
            gap: 10,
            background: "var(--panel2)",
            border: `1px solid ${danger ? "rgba(232,82,94,.4)" : "var(--line2)"}`,
            borderRadius: 10,
            padding: "11px 14px",
            boxShadow: "0 14px 34px rgba(0,0,0,.5)",
            maxWidth: 380,
          }}
        >
          <span
            style={{
              width: 7,
              height: 7,
              borderRadius: "50%",
              background: danger ? "var(--dgr)" : "var(--ok)",
              flex: "0 0 7px",
            }}
          />
          <span style={{ fontSize: 13, color: "var(--tx)" }}>{toast.msg}</span>
        </div>
      )}
    </ToastContext.Provider>
  );
}
