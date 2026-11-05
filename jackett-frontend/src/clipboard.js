// Clipboard helper that also works on plain http:// LAN origins where
// navigator.clipboard is unavailable (falls back to execCommand).
export async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    try {
      return document.execCommand("copy");
    } finally {
      ta.remove();
    }
  }
}
