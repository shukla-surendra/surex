import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from 'react';

type Notify = (message: string) => void;

const ToastContext = createContext<Notify>(() => {});

export function ToastProvider({ children }: { children: ReactNode }) {
  const [message, setMessage] = useState<string | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const notify = useCallback<Notify>((msg) => {
    setMessage(msg);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setMessage(null), 3000);
  }, []);
  return (
    <ToastContext.Provider value={notify}>
      {children}
      {message && (
        <div className="toast" role="alert">
          {message}
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
        notify(e instanceof Error ? e.message : String(e));
      }
    },
    [notify],
  );
}

export function useToast(): Notify {
  return useContext(ToastContext);
}
