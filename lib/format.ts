export function formatRelative(ms: number): string {
  if (!ms) return "—";
  const diff = Date.now() - ms;
  const abs = Math.abs(diff);
  const sec = Math.round(abs / 1000);
  const min = Math.round(sec / 60);
  const hr = Math.round(min / 60);
  const day = Math.round(hr / 24);
  if (sec < 60) return `${sec}秒前`;
  if (min < 60) return `${min}分前`;
  if (hr < 24) return `${hr}時間前`;
  return `${day}日前`;
}
