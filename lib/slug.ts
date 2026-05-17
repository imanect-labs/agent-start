export function slugify(name: string): string {
  const cleaned = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return cleaned || "project";
}

export function sessionName(prefix: string, projectName: string): string {
  const slug = slugify(projectName).slice(0, 32);
  const ts = Math.floor(Date.now() / 1000);
  return `${prefix}${slug}-${ts}`;
}
