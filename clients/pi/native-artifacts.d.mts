export type NativeTargetKey = 'darwin-arm64' | 'darwin-x64' | 'linux-arm64' | 'linux-x64';
export type NativeArtifactKind = 'platform_unsupported' | 'artifact_missing' | 'artifact_unreadable' | 'artifact_mismatch' | 'inspection_unknown';
export const MAX_NATIVE_BYTES: number;
export const NATIVE_TARGETS: Readonly<Record<NativeTargetKey, Readonly<{ target: string; format: 'macho' | 'elf'; cpu: number; path: string }>>>;
export type NativeManifestEntry = Readonly<{ target: string; path: string; size: number; sha256: string }>;
export type NativeManifest = Readonly<{ schema: 1; targets: Partial<Record<NativeTargetKey, NativeManifestEntry>> }>;
export class NativeArtifactFailure extends Error {
  constructor(kind?: NativeArtifactKind);
  readonly kind: NativeArtifactKind;
  readonly diagnostic: Readonly<{ code: string; stage: 'helper_start'; constraint: string; action: string }>;
}
export function nativeArtifactDiagnostic(error: unknown): NativeArtifactFailure['diagnostic'];
export function validateNativeManifest(value: unknown, requireAll?: boolean): NativeManifest;
export function inspectNativeHeader(bytes: Buffer, key: NativeTargetKey): void;
export function resolveNativeHelper(base?: URL, platform?: string, arch?: string): string;
