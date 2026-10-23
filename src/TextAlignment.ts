export const TEXT_ALIGN_VALUES = ['left', 'center', 'right'] as const;

export type TextAlign = (typeof TEXT_ALIGN_VALUES)[number];

export function isTextAlign(value: unknown): value is TextAlign {
    return typeof value === 'string' && TEXT_ALIGN_VALUES.some((align) => align === value);
}

export function isTextAlignState(value: unknown): value is TextAlign | 'mixed' | null {
    return value === null || value === 'mixed' || isTextAlign(value);
}
