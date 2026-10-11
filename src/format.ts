export function gb(mb: number): string {
  return (Math.round((mb / 1024) * 10) / 10).toFixed(1)
}
