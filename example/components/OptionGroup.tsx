import { Pressable, StyleSheet, Text, View } from 'react-native';

import { FONT_SIZE, LINE_HEIGHT, PALETTE, RADIUS, SPACE } from '../theme';

interface OptionGroupProps<Option extends string> {
    label: string;
    options: readonly Option[];
    labels: Readonly<Record<Option, string>>;
    value: Option;
    onChange: (option: Option) => void;
}

export function OptionGroup<Option extends string>({
    label,
    options,
    labels,
    value,
    onChange,
}: OptionGroupProps<Option>) {
    return (
        <View accessibilityRole={'radiogroup'} accessibilityLabel={label} style={styles.group}>
            {options.map(option => {
                const selected = option === value;

                return (
                    <Pressable
                        key={option}
                        accessibilityRole={'radio'}
                        accessibilityState={{ selected }}
                        hitSlop={SPACE.sm}
                        onPress={() => onChange(option)}
                        style={[ styles.chip, selected && styles.chipSelected ]}
                    >
                        <Text style={[ styles.chipText, selected && styles.chipTextSelected ]}>
                            {labels[option]}
                        </Text>
                    </Pressable>
                );
            })}
        </View>
    );
}

const styles = StyleSheet.create({
    group: {
        flexDirection: 'row',
        gap: SPACE.xs,
    },
    chip: {
        paddingHorizontal: SPACE.md,
        paddingVertical: SPACE.xs,
        borderRadius: RADIUS.control,
        backgroundColor: PALETTE.spruce,
    },
    chipSelected: {
        backgroundColor: PALETTE.spruceTint,
    },
    chipText: {
        color: PALETTE.paper,
        fontSize: FONT_SIZE.caption,
        lineHeight: LINE_HEIGHT.caption,
        fontWeight: '600',
    },
    chipTextSelected: {
        color: PALETTE.spruceDeep,
    },
});
