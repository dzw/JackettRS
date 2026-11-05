export function fmtSize(n) {
  n = Number(n);
  if (!n || n < 0) return "—";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return n.toFixed(i ? 1 : 0) + " " + u[i];
}

export function fmtDate(s) {
  if (!s) return "—";
  const d = new Date(s);
  return isNaN(d) ? String(s) : d.toLocaleString();
}
