/** A path relative to the project root. A path outside the root comes back
 * unchanged, so the backend refuses it rather than acting on a namesake. */
export function relativePath(root: string, path: string): string {
  const prefix = root.endsWith('/') ? root : `${root}/`;
  return path.startsWith(prefix) ? path.slice(prefix.length) : path;
}

export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
