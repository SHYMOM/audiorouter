import { useEffect, useState } from "react";
import { Icon } from "./icons";

const iconCache = new Map<string, string>();
const failedCache = new Set<string>();

/** Fetches and caches small Windows `.exe` icons from the Tauri shell. */
export function useApplicationIcons(paths: readonly (string | null | undefined)[]): ReadonlyMap<string, string> {
  const [, setVersion] = useState(0);

  const missing: string[] = [];
  const seen = new Set<string>();
  for (const path of paths) {
    if (!path) continue;
    const key = path.toLowerCase();
    if (seen.has(key) || iconCache.has(key) || failedCache.has(key)) continue;
    seen.add(key);
    missing.push(path);
  }
  const missingKey = missing.join("\0");

  useEffect(() => {
    if (!missingKey) return;
    const invoke = window.__TAURI_INTERNALS__?.invoke;
    if (!invoke) return;
    let cancelled = false;
    const batch = missingKey.split("\0");
    void invoke("get_application_icons", { paths: batch })
      .then((result) => {
        if (cancelled || !result || typeof result !== "object") return;
        const map = result as Record<string, string>;
        for (const path of batch) {
          const key = path.toLowerCase();
          const url = map[path];
          if (typeof url === "string" && url.startsWith("data:image/")) {
            iconCache.set(key, url);
          } else {
            failedCache.add(key);
          }
        }
        setVersion((v) => v + 1);
      })
      .catch(() => {
        for (const path of batch) failedCache.add(path.toLowerCase());
      });
    return () => {
      cancelled = true;
    };
  }, [missingKey]);

  return iconCache;
}

export function AppExeIcon({
  executablePath,
  icons,
}: {
  executablePath?: string | null;
  icons: ReadonlyMap<string, string>;
}) {
  const [broken, setBroken] = useState(false);
  const src = executablePath ? icons.get(executablePath.toLowerCase()) : undefined;
  if (src && !broken) {
    return (
      <img
        className="app-exe-icon"
        src={src}
        width={16}
        height={16}
        alt=""
        aria-hidden="true"
        onError={() => setBroken(true)}
      />
    );
  }
  return <Icon name="app-window" size={16} />;
}
