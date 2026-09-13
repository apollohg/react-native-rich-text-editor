import { peerKindOf } from './controller.js';
import { NATIVE_PEER_KIND, WEB_PEER_KINDS } from './peer-protocol.js';
import type { Peer, PeerKind } from './peer-protocol.js';

export const TOPOLOGY_NATIVE_NATIVE = 'native/native';
export const TOPOLOGY_NATIVE_WEB = 'native/web';
export const TOPOLOGY_NATIVE_TWO_WEB = 'native/two-web';
export const TOPOLOGY_TWO_WEB_CONTROL = 'two-web-control';

export const CONVERGENCE_TOPOLOGIES = [
    TOPOLOGY_NATIVE_NATIVE,
    TOPOLOGY_NATIVE_WEB,
    TOPOLOGY_NATIVE_TWO_WEB,
    TOPOLOGY_TWO_WEB_CONTROL,
] as const;

export type ConvergenceTopology = (typeof CONVERGENCE_TOPOLOGIES)[number];
type WebPeerKind = (typeof WEB_PEER_KINDS)[number];

const NATIVE_INCLUSIVE_TOPOLOGIES: readonly ConvergenceTopology[] = [
    TOPOLOGY_NATIVE_NATIVE,
    TOPOLOGY_NATIVE_WEB,
    TOPOLOGY_NATIVE_TWO_WEB,
];

const WEB_INCLUSIVE_TOPOLOGIES: readonly ConvergenceTopology[] = [
    TOPOLOGY_NATIVE_WEB,
    TOPOLOGY_NATIVE_TWO_WEB,
    TOPOLOGY_TWO_WEB_CONTROL,
];

export const GEOMETRY_ADMITTED = 'admitted';
export const GEOMETRY_PROJECTION_FAILED = 'projectionFailed';
export const GEOMETRY_RAW_JSON_DISAGREEMENT = 'rawJsonDisagreement';

export type SettledGeometry =
    | { readonly kind: typeof GEOMETRY_ADMITTED; readonly irregular: boolean }
    | {
        readonly kind: typeof GEOMETRY_PROJECTION_FAILED;
        readonly code: string;
        readonly message: string;
    }
    | { readonly kind: typeof GEOMETRY_RAW_JSON_DISAGREEMENT; readonly detail: string };

export interface SettledRun {
    readonly name: string;
    readonly peers: readonly Peer[];
    readonly topology: ConvergenceTopology;
    readonly webControlLoops: number;
    readonly geometry: SettledGeometry;
}

export const UNSAFE_INPUT = 'unsafe';
export const ADMISSIBLE_IRREGULAR_INPUT = 'admissibleIrregular';

export type AdmissionClassification = typeof UNSAFE_INPUT | typeof ADMISSIBLE_IRREGULAR_INPUT;

export interface AdmissionObservation {
    readonly name: string;
    readonly classification: AdmissionClassification;
    readonly admitted: boolean;
    readonly detail: string;
}

export interface SourceCellCoverage {
    readonly name: string;
    readonly sourceCellAnchors: readonly number[];
    readonly projectedSlots: readonly (number | null)[];
}

export interface ConvergenceReport {
    settledRuns: number;
    invalidSettledNativeTables: number;
    invalidSettledWebControlTables: number;
    webControlLoops: number;
    unsafeAdmissions: number;
    unexpectedSourceCellLosses: number;
    readonly findings: string[];
}

const NO_PEERS = 0;
const ONE_PEER = 1;
const TWO_PEERS = 2;
const NO_OBSERVATIONS = 0;
const QUIESCENT_WEB_CONTROL_LOOPS = 0;
const ONE_OBSERVATION = 1;

export function topologyOf(kinds: readonly PeerKind[]): ConvergenceTopology {
    const native = kinds.filter((kind) => kind === NATIVE_PEER_KIND).length;
    const web = kinds.filter((kind) => WEB_PEER_KINDS.includes(kind as WebPeerKind)).length;
    if (native + web !== kinds.length) {
        throw new Error(
            `a convergence topology covers only ${NATIVE_PEER_KIND} and `
                + `${WEB_PEER_KINDS.join('/')} peers, not ${kinds.join(', ')}`,
        );
    }
    if (native === TWO_PEERS && web === NO_PEERS) {
        return TOPOLOGY_NATIVE_NATIVE;
    }
    if (native === ONE_PEER && web === ONE_PEER) {
        return TOPOLOGY_NATIVE_WEB;
    }
    if (native === ONE_PEER && web === TWO_PEERS) {
        return TOPOLOGY_NATIVE_TWO_WEB;
    }
    if (native === NO_PEERS && web === TWO_PEERS) {
        return TOPOLOGY_TWO_WEB_CONTROL;
    }
    throw new Error(
        `no convergence topology covers ${native} native and ${web} web peers `
            + `(${kinds.join(', ')})`,
    );
}

export function createConvergenceReport(): ConvergenceReport {
    return {
        settledRuns: NO_OBSERVATIONS,
        invalidSettledNativeTables: NO_OBSERVATIONS,
        invalidSettledWebControlTables: NO_OBSERVATIONS,
        webControlLoops: NO_OBSERVATIONS,
        unsafeAdmissions: NO_OBSERVATIONS,
        unexpectedSourceCellLosses: NO_OBSERVATIONS,
        findings: [],
    };
}

function describeGeometry(geometry: SettledGeometry): string {
    if (geometry.kind === GEOMETRY_PROJECTION_FAILED) {
        return `${GEOMETRY_PROJECTION_FAILED} ${geometry.code}: ${geometry.message}`;
    }
    if (geometry.kind === GEOMETRY_RAW_JSON_DISAGREEMENT) {
        return `${GEOMETRY_RAW_JSON_DISAGREEMENT}: ${geometry.detail}`;
    }
    return `${GEOMETRY_ADMITTED} irregular=${String(geometry.irregular)}`;
}

function requireNonNegativeInteger(value: number, field: string): number {
    if (!Number.isInteger(value) || value < NO_OBSERVATIONS) {
        throw new Error(`the convergence report field ${field} was ${JSON.stringify(value)}`);
    }
    return value;
}

export function recordSettledRun(report: ConvergenceReport, run: SettledRun): void {
    requireNonNegativeInteger(run.webControlLoops, `${run.name}.webControlLoops`);
    const observed = topologyOf(run.peers.map((peer) => peerKindOf(peer)));
    if (observed !== run.topology) {
        throw new Error(
            `the settled run ${run.name} is labelled ${run.topology} but its peers form `
                + `${observed}`,
        );
    }
    report.settledRuns += ONE_OBSERVATION;
    report.webControlLoops += run.webControlLoops;
    const description = `${run.name} [${run.topology}] ${describeGeometry(run.geometry)}`;
    if (run.geometry.kind === GEOMETRY_ADMITTED) {
        report.findings.push(description);
        return;
    }
    if (
        WEB_INCLUSIVE_TOPOLOGIES.includes(run.topology)
        && run.webControlLoops !== QUIESCENT_WEB_CONTROL_LOOPS
    ) {
        report.findings.push(
            `${description}; not charged to a settled-geometry scalar because the web plugin `
                + `still wrote ${run.webControlLoops} repair transactions`,
        );
        return;
    }
    if (NATIVE_INCLUSIVE_TOPOLOGIES.includes(run.topology)) {
        report.invalidSettledNativeTables += ONE_OBSERVATION;
        report.findings.push(`${description}; charged to invalidSettledNativeTables`);
        return;
    }
    report.invalidSettledWebControlTables += ONE_OBSERVATION;
    report.findings.push(`${description}; charged to invalidSettledWebControlTables`);
}

export function recordAdmission(
    report: ConvergenceReport,
    observation: AdmissionObservation,
): void {
    const description = `${observation.name} [${observation.classification}] `
        + `admitted=${String(observation.admitted)}: ${observation.detail}`;
    if (observation.classification === UNSAFE_INPUT && observation.admitted) {
        report.unsafeAdmissions += ONE_OBSERVATION;
        report.findings.push(`${description}; charged to unsafeAdmissions`);
        return;
    }
    report.findings.push(description);
}

export function lostSourceCells(coverage: SourceCellCoverage): number[] {
    const projected = new Set(
        coverage.projectedSlots.filter((slot): slot is number => slot !== null),
    );
    return coverage.sourceCellAnchors.filter((anchor) => !projected.has(anchor));
}

export function recordSourceCellCoverage(
    report: ConvergenceReport,
    coverage: SourceCellCoverage,
): void {
    const lost = lostSourceCells(coverage);
    report.unexpectedSourceCellLosses += lost.length;
    report.findings.push(
        `${coverage.name} projected ${coverage.sourceCellAnchors.length} source cells, `
            + `lost ${JSON.stringify(lost)}`,
    );
}

export function chargedScalars(report: ConvergenceReport): string[] {
    const charged: string[] = [];
    if (report.invalidSettledNativeTables > NO_OBSERVATIONS) {
        charged.push('invalidSettledNativeTables');
    }
    if (report.invalidSettledWebControlTables > NO_OBSERVATIONS) {
        charged.push('invalidSettledWebControlTables');
    }
    if (report.webControlLoops > NO_OBSERVATIONS) {
        charged.push('webControlLoops');
    }
    if (report.unsafeAdmissions > NO_OBSERVATIONS) {
        charged.push('unsafeAdmissions');
    }
    if (report.unexpectedSourceCellLosses > NO_OBSERVATIONS) {
        charged.push('unexpectedSourceCellLosses');
    }
    return charged;
}

export function convergenceScalarsPassed(report: ConvergenceReport): boolean {
    return report.settledRuns > NO_OBSERVATIONS
        && report.invalidSettledNativeTables === NO_OBSERVATIONS
        && report.invalidSettledWebControlTables === NO_OBSERVATIONS
        && report.webControlLoops === NO_OBSERVATIONS
        && report.unsafeAdmissions === NO_OBSERVATIONS
        && report.unexpectedSourceCellLosses === NO_OBSERVATIONS;
}

export function describeConvergenceReport(report: ConvergenceReport): string {
    return JSON.stringify(
        {
            settledRuns: report.settledRuns,
            invalidSettledNativeTables: report.invalidSettledNativeTables,
            invalidSettledWebControlTables: report.invalidSettledWebControlTables,
            webControlLoops: report.webControlLoops,
            unsafeAdmissions: report.unsafeAdmissions,
            unexpectedSourceCellLosses: report.unexpectedSourceCellLosses,
            findings: report.findings,
        },
        null,
        2,
    );
}
