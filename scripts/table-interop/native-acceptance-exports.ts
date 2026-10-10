export const EXPORTS_VARIABLE = 'NATIVE_TABLE_ACCEPTANCE_EXPORTS';

const EXPORT_SEPARATOR = ',';

export function nativeAcceptanceExportPaths(): string[] | null {
    const listed = process.env[EXPORTS_VARIABLE];
    if (listed === undefined) {
        return null;
    }
    const paths = listed
        .split(EXPORT_SEPARATOR)
        .map((path) => path.trim())
        .filter((path) => path.length > 0);
    if (paths.length === 0) {
        throw new Error(`${EXPORTS_VARIABLE} is set but lists no export paths: ${JSON.stringify(listed)}`);
    }
    return paths;
}

export function requireNativeAcceptanceExportPaths(): string[] {
    const paths = nativeAcceptanceExportPaths();
    if (paths === null) {
        throw new Error(`${EXPORTS_VARIABLE} must list the native acceptance exports to compare`);
    }
    return paths;
}
