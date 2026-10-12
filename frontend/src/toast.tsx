import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from 'react';

type Kind = 'info' | 'error';
type Notify = (message: string, kind?: Kind) => void;

const ToastContext = createContext<Notify>(() => {});

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toast, setToast] = useState<{ message: string; kind: Kind } | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const notify = useCallback<Notify>((message, kind = 'info') => {
    setToast({ message, kind });
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setToast(null), 3000);
  }, []);
  return (
    <ToastContext.Provider value={notify}>
      {children}
      {toast && (
        <div className={`toast ${toast.kind}`} role={toast.kind === 'error' ? 'alert' : 'status'}>
          {toast.message}
        </div>
      )}
    </ToastContext.Provider>
  );
}

/** Returns a wrapper that runs an async action and shows any error as a toast. */
export function useGuard() {
  const notify = useContext(ToastContext);
  return useCallback(
    async (action: () => Promise<unknown>) => {
      try {
        await action();
      } catch (e) {
        notify(e instanceof Error ? e.message : String(e), 'error');
      }
    },
    [notify],
  );
}

export function useToast(): Notify {
  return useContext(ToastContext);
}
