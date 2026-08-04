export function toLocalDay(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function fromLocalDay(day: string): Date {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!match) throw new Error(`Invalid local day: ${day}`);

  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  if (toLocalDay(date) !== day) throw new Error(`Invalid local day: ${day}`);
  return date;
}

export function addLocalDays(day: string, amount: number): string {
  const date = fromLocalDay(day);
  date.setDate(date.getDate() + amount);
  return toLocalDay(date);
}

export function formatLocalDay(day: string): string {
  return new Intl.DateTimeFormat(undefined, {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
  }).format(fromLocalDay(day));
}

export function millisecondsUntilNextLocalDay(now: Date): number {
  const next = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  return next.getTime() - now.getTime();
}
