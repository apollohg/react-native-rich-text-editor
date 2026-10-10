import { readFileSync } from 'node:fs';

export function parseJson(text, label) {
    let payload;
    try {
        payload = JSON.parse(text);
    } catch (error) {
        throw new Error(`failed to parse ${label} JSON: ${error.message}`);
    }
    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
        throw new Error(`${label} must be a JSON object`);
    }
    return payload;
}

export function readJsonFile(filePath, label) {
    let text;
    try {
        text = readFileSync(filePath, 'utf8');
    } catch (error) {
        throw new Error(`failed to read ${label} file ${filePath}: ${error.message}`);
    }
    return parseJson(text, label);
}
