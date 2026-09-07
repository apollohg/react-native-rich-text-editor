const path = require('path');
const { createRequire } = require('module');

const pkg = require('./package.json');

const PACKAGING_EXCLUDES_PROPERTY = 'android.packagingOptions.excludes';

const LEGACY_JNA_ABI_EXCLUDES = [
    '**/armeabi/libjnidispatch.so',
    '**/mips/libjnidispatch.so',
    '**/mips64/libjnidispatch.so',
];

function requireExpoConfigPlugins() {
    try {
        return require('expo/config-plugins');
    } catch (error) {
        if (error.code !== 'MODULE_NOT_FOUND') {
            throw error;
        }

        const appRequire = createRequire(path.join(process.cwd(), 'package.json'));

        return appRequire('expo/config-plugins');
    }
}

function mergeCommaList(value, additions) {
    const values = (value || '')
        .split(',')
        .map(item => item.trim())
        .filter(Boolean);

    return Array.from(new Set([ ...values, ...additions ])).join(',');
}

function withReactNativeProseEditor(config) {
    const { withGradleProperties } = requireExpoConfigPlugins();

    return withGradleProperties(config, config => {
        const existing = config.modResults.find(
            property => property.type === 'property' && property.key === PACKAGING_EXCLUDES_PROPERTY
        );

        const value = mergeCommaList(existing && existing.value, LEGACY_JNA_ABI_EXCLUDES);

        if (existing) {
            existing.value = value;
        } else {
            config.modResults.push({
                type: 'property',
                key: PACKAGING_EXCLUDES_PROPERTY,
                value,
            });
        }

        return config;
    });
}

module.exports = requireExpoConfigPlugins().createRunOncePlugin(
    withReactNativeProseEditor,
    pkg.name,
    pkg.version
);
