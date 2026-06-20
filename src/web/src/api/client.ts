import { createBLAKE3 } from "hash-wasm";

export type ApiErrorCode = string;

export type ApiErrorBody = {
  error: ApiErrorCode;
  message: string;
};

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, body: ApiErrorBody) {
    super(body.message);
    this.status = status;
    this.code = body.error;
  }
}

export type SetupOwnerInput = {
  setupToken: string;
  displayName: string;
  password: string;
};

export async function setupOwner(input: SetupOwnerInput): Promise<void> {
  await request("/setup/owner", {
    method: "POST",
    body: jsonBody({
      setup_token: input.setupToken,
      display_name: input.displayName,
      password: input.password
    })
  });
}

export type LoginInput = {
  password: string;
  deviceName?: string;
  totpCode?: string;
  recoveryCode?: string;
};

export async function login(input: LoginInput): Promise<void> {
  await request("/auth/login", {
    method: "POST",
    body: jsonBody({
      password: input.password,
      device_name: input.deviceName ?? null,
      totp_code: blankToNull(input.totpCode),
      recovery_code: blankToNull(input.recoveryCode)
    })
  });
}

export async function logout(): Promise<void> {
  await request("/auth/logout", {
    method: "POST",
    headers: csrfHeaders()
  });
}

export type HealthStatus = {
  status: string;
};

export type ReadyStatus = {
  status: string;
  config: string;
  database: string;
};

export async function getHealth(): Promise<HealthStatus> {
  const response = await request("/health", { method: "GET" });
  return (await response.json()) as HealthStatus;
}

export async function getReady(): Promise<ReadyStatus> {
  const response = await request("/ready", { method: "GET" });
  return (await response.json()) as ReadyStatus;
}

export type Session = {
  sessionId: string;
  deviceName: string | null;
  userAgent: string | null;
  createdAt: string;
  lastSeenAt: string | null;
  expiresAt: string;
  isCurrent: boolean;
};

type SessionDto = {
  session_id: string;
  device_name: string | null;
  user_agent: string | null;
  created_at: string;
  last_seen_at: string | null;
  expires_at: string;
  is_current: boolean;
};

export async function listSessions(): Promise<Session[]> {
  const response = await request("/sessions", { method: "GET" });
  const body = (await response.json()) as SessionDto[];

  return body.map(mapSession);
}

export type MfaStatus = {
  totpEnabled: boolean;
  totpSetupPending: boolean;
  recoveryCodesRemaining: number;
};

type MfaStatusDto = {
  totp_enabled: boolean;
  totp_setup_pending: boolean;
  recovery_codes_remaining: number;
};

export async function getMfaStatus(): Promise<MfaStatus> {
  const response = await request("/auth/mfa", { method: "GET" });
  const body = (await response.json()) as MfaStatusDto;
  return {
    totpEnabled: body.totp_enabled,
    totpSetupPending: body.totp_setup_pending,
    recoveryCodesRemaining: body.recovery_codes_remaining
  };
}

export type TotpSetup = {
  secretBase32: string;
  provisioningUri: string;
};

type TotpSetupDto = {
  secret_base32: string;
  provisioning_uri: string;
};

export async function setupTotp(password: string): Promise<TotpSetup> {
  const response = await request("/auth/totp/setup", {
    method: "POST",
    headers: csrfHeaders(),
    body: jsonBody({ password })
  });
  const body = (await response.json()) as TotpSetupDto;
  return {
    secretBase32: body.secret_base32,
    provisioningUri: body.provisioning_uri
  };
}

export async function enableTotp(input: { password: string; totpCode: string }): Promise<string[]> {
  const response = await request("/auth/totp/enable", {
    method: "POST",
    headers: csrfHeaders(),
    body: jsonBody({
      password: input.password,
      totp_code: input.totpCode
    })
  });
  const body = (await response.json()) as { recovery_codes: string[] };
  return body.recovery_codes;
}

export async function disableTotp(input: SecondFactorInput): Promise<void> {
  await request("/auth/totp/disable", {
    method: "POST",
    headers: csrfHeaders(),
    body: jsonBody(secondFactorBody(input))
  });
}

export async function rotateRecoveryCodes(input: SecondFactorInput): Promise<string[]> {
  const response = await request("/auth/recovery-codes/rotate", {
    method: "POST",
    headers: csrfHeaders(),
    body: jsonBody(secondFactorBody(input))
  });
  const body = (await response.json()) as { recovery_codes: string[] };
  return body.recovery_codes;
}

export type SecondFactorInput = {
  password: string;
  totpCode?: string;
  recoveryCode?: string;
};

export type DeviceTokenOutput = {
  deviceTokenId: string;
  token: string;
};

export async function createDeviceToken(input: {
  name: string;
  password: string;
  totpCode?: string;
  recoveryCode?: string;
}): Promise<DeviceTokenOutput> {
  const response = await request("/device-tokens", {
    method: "POST",
    headers: csrfHeaders(),
    body: jsonBody({
      name: input.name,
      password: input.password,
      totp_code: blankToNull(input.totpCode),
      recovery_code: blankToNull(input.recoveryCode)
    })
  });
  const body = (await response.json()) as { device_token_id: string; token: string };
  return {
    deviceTokenId: body.device_token_id,
    token: body.token
  };
}

export async function revokeDeviceToken(deviceTokenId: string): Promise<void> {
  await request(`/device-tokens/${deviceTokenId}`, {
    method: "DELETE",
    headers: csrfHeaders()
  });
}

export type AssetDerivative = {
  format: string;
  width: number;
  height: number;
};

export type AssetTimelineItem = {
  assetId: string;
  createdAt: string;
  trashedAt?: string;
  favoriteAt: string | null;
  originalBlake3: string;
  mediaType: string;
  sizeBytes: number;
  originalFilename: string | null;
  thumbnail: AssetDerivative | null;
  preview: AssetDerivative | null;
};

type AssetTimelineItemDto = {
  asset_id: string;
  created_at: string;
  trashed_at?: string;
  favorite_at: string | null;
  original_blake3: string;
  media_type: string;
  size_bytes: number;
  original_filename: string | null;
  thumbnail: AssetDerivative | null;
  preview: AssetDerivative | null;
};

type AssetTimelinePageDto = {
  items: AssetTimelineItemDto[];
  next_cursor: string | null;
};

export type AssetTimelinePage = {
  items: AssetTimelineItem[];
  nextCursor: string | null;
};

export async function listAssets(cursor?: string): Promise<AssetTimelinePage> {
  return listAssetPage("/assets", cursor);
}

export async function listTrashedAssets(cursor?: string): Promise<AssetTimelinePage> {
  return listAssetPage("/trash/assets", cursor);
}

export async function searchAssets(input: {
  query: string;
  mode: "filename" | "semantic";
  limit?: number;
}): Promise<AssetTimelinePage> {
  const params = new URLSearchParams({
    q: input.query,
    mode: input.mode,
    limit: String(input.limit ?? 40)
  });
  const response = await request(`/search?${params.toString()}`, { method: "GET" });
  return mapAssetPage((await response.json()) as AssetTimelinePageDto);
}

export async function favoriteAsset(assetId: string): Promise<void> {
  await request(`/assets/${assetId}/favorite`, {
    method: "POST",
    headers: csrfHeaders()
  });
}

export async function unfavoriteAsset(assetId: string): Promise<void> {
  await request(`/assets/${assetId}/favorite`, {
    method: "DELETE",
    headers: csrfHeaders()
  });
}

export async function trashAsset(assetId: string): Promise<void> {
  await request(`/assets/${assetId}`, {
    method: "DELETE",
    headers: csrfHeaders()
  });
}

export async function restoreAsset(assetId: string): Promise<void> {
  await request(`/assets/${assetId}/restore`, {
    method: "POST",
    headers: csrfHeaders()
  });
}

export async function purgeAsset(assetId: string): Promise<void> {
  await request(`/assets/${assetId}/purge`, {
    method: "DELETE",
    headers: csrfHeaders()
  });
}

export function derivativeUrl(assetId: string, kind: "thumbnail" | "preview"): string {
  return `/assets/${assetId}/derivatives/${kind}`;
}

export type UploadSession = {
  uploadId: string;
  status: string;
  expectedSize: number;
  expectedBlake3: string;
  mediaType: string;
  committedParts: number[];
};

type UploadSessionDto = {
  upload_id: string;
  status: string;
  expected_size: number;
  expected_blake3: string;
  media_type: string;
  committed_parts: number[];
};

export type CompleteUploadResult = {
  upload: UploadSession;
  promoted: {
    assetId: string;
  };
};

type CompleteUploadDto = {
  upload: UploadSessionDto;
  promoted: {
    asset_id: string;
  };
};

export type UploadProgress = {
  phase: "hashing" | "creating" | "uploading" | "completing" | "done";
  loadedBytes: number;
  totalBytes: number;
};

const uploadPartSizeBytes = 4 * 1024 * 1024;

export async function uploadAsset(
  file: File,
  onProgress?: (progress: UploadProgress) => void
): Promise<CompleteUploadResult> {
  onProgress?.({ phase: "hashing", loadedBytes: 0, totalBytes: file.size });
  const expectedBlake3 = await hashFileBlake3(file, (loadedBytes) => {
    onProgress?.({ phase: "hashing", loadedBytes, totalBytes: file.size });
  });

  onProgress?.({ phase: "creating", loadedBytes: 0, totalBytes: file.size });
  const upload = await createUpload({
    originalFilename: file.name,
    expectedSize: file.size,
    expectedBlake3,
    mediaType: file.type || "application/octet-stream",
    clientUploadKey: makeClientUploadKey()
  });
  const committed = new Set(upload.committedParts);
  let uploadedBytes = 0;

  for (let partIndex = 0, offset = 0; offset < file.size; partIndex += 1, offset += uploadPartSizeBytes) {
    const end = Math.min(offset + uploadPartSizeBytes, file.size);
    if (!committed.has(partIndex)) {
      const bytes = await file.slice(offset, end).arrayBuffer();
      await putUploadPart(upload.uploadId, partIndex, bytes);
    }
    uploadedBytes = end;
    onProgress?.({ phase: "uploading", loadedBytes: uploadedBytes, totalBytes: file.size });
  }

  onProgress?.({ phase: "completing", loadedBytes: file.size, totalBytes: file.size });
  const result = await completeUpload(upload.uploadId);
  onProgress?.({ phase: "done", loadedBytes: file.size, totalBytes: file.size });
  return result;
}

export type ExportManifest = {
  generatedAt: string;
  manifestVersion: string;
  items: ExportManifestItem[];
};

export type ExportManifestItem = {
  assetId: string;
  blake3Hash: string;
  storageKey: string;
  mediaType: string;
  sizeBytes: number;
  originalFilename: string | null;
  createdAt: string;
};

type ExportManifestDto = {
  generated_at: string;
  manifest_version: string;
  items: ExportManifestItemDto[];
};

type ExportManifestItemDto = {
  asset_id: string;
  blake3_hash: string;
  storage_key: string;
  media_type: string;
  size_bytes: number;
  original_filename: string | null;
  created_at: string;
};

export async function getExportManifest(): Promise<ExportManifest> {
  const response = await request("/exports/originals/manifest", { method: "GET" });
  const body = (await response.json()) as ExportManifestDto;
  return {
    generatedAt: body.generated_at,
    manifestVersion: body.manifest_version,
    items: body.items.map((item) => ({
      assetId: item.asset_id,
      blake3Hash: item.blake3_hash,
      storageKey: item.storage_key,
      mediaType: item.media_type,
      sizeBytes: item.size_bytes,
      originalFilename: item.original_filename,
      createdAt: item.created_at
    }))
  };
}

export type PersonSummary = {
  personId: string;
  displayName: string | null;
  reviewStatus: string;
  faceCount: number;
};

type PersonSummaryDto = {
  person_id: string;
  display_name: string | null;
  review_status: string;
  face_count: number;
};

export async function listPeople(): Promise<PersonSummary[]> {
  const response = await request("/people", { method: "GET" });
  const body = (await response.json()) as PersonSummaryDto[];
  return body.map((person) => ({
    personId: person.person_id,
    displayName: person.display_name,
    reviewStatus: person.review_status,
    faceCount: person.face_count
  }));
}

export type FaceAlbumItem = {
  faceId: string;
  assetId: string;
  assetCreatedAt: string;
  mediaType: string;
  reviewState: string;
  chipAvailable: boolean;
};

type FaceAlbumItemDto = {
  face_id: string;
  asset_id: string;
  asset_created_at: string;
  media_type: string;
  review_state: string;
  chip_available: boolean;
};

export async function listUnassignedFaces(limit = 24): Promise<FaceAlbumItem[]> {
  const response = await request(`/people/faces/unassigned?limit=${String(limit)}`, {
    method: "GET"
  });
  const body = (await response.json()) as FaceAlbumItemDto[];
  return body.map((face) => ({
    faceId: face.face_id,
    assetId: face.asset_id,
    assetCreatedAt: face.asset_created_at,
    mediaType: face.media_type,
    reviewState: face.review_state,
    chipAvailable: face.chip_available
  }));
}

export function faceChipUrl(faceId: string): string {
  return `/people/faces/${faceId}/chip`;
}

export type ModelPackSummary = {
  modelPackId: string;
  kind: string;
  runtime: string;
  modelKey: string;
  modelRevision: string;
  status: string;
  selfTestStatus: string;
  embeddingDimension: number;
  distanceMetric: string;
  updatedAt: string;
};

type ModelPackSummaryDto = {
  model_pack_id: string;
  kind: string;
  runtime: string;
  model_key: string;
  model_revision: string;
  status: string;
  self_test_status: string;
  embedding_dimension: number;
  distance_metric: string;
  updated_at: string;
};

export async function listModelPacks(): Promise<ModelPackSummary[]> {
  const response = await request("/model-packs", { method: "GET" });
  const body = (await response.json()) as ModelPackSummaryDto[];
  return body.map(mapModelPack);
}

export async function installModelPack(manifest: unknown): Promise<ModelPackSummary> {
  const response = await request("/model-packs", {
    method: "POST",
    headers: csrfHeaders(),
    body: jsonBody(manifest)
  });
  return mapModelPack((await response.json()) as ModelPackSummaryDto);
}

export async function runModelPackSelfTest(modelPackId: string): Promise<ModelPackSummary> {
  const response = await request(`/model-packs/${modelPackId}/self-test/run`, {
    method: "POST",
    headers: csrfHeaders()
  });
  return mapModelPack((await response.json()) as ModelPackSummaryDto);
}

export async function activateModelPack(modelPackId: string): Promise<ModelPackSummary> {
  const response = await request(`/model-packs/${modelPackId}/activate`, {
    method: "POST",
    headers: csrfHeaders()
  });
  return mapModelPack((await response.json()) as ModelPackSummaryDto);
}

export type ModelReindexRun = {
  reindexRunId: string;
  modelPackId: string;
  status: string;
  totalAssets: number;
  queuedAssets: number;
  processedAssets: number;
  failedAssets: number;
};

type ModelReindexRunDto = {
  reindex_run_id: string;
  model_pack_id: string;
  status: string;
  total_assets: number;
  queued_assets: number;
  processed_assets: number;
  failed_assets: number;
};

export async function startModelReindex(modelPackId: string): Promise<ModelReindexRun> {
  const response = await request(`/model-packs/${modelPackId}/reindex`, {
    method: "POST",
    headers: csrfHeaders()
  });
  return mapReindexRun((await response.json()) as ModelReindexRunDto);
}

export async function listModelReindexRuns(modelPackId: string): Promise<ModelReindexRun[]> {
  const response = await request(`/model-packs/${modelPackId}/reindex-runs`, {
    method: "GET"
  });
  const body = (await response.json()) as ModelReindexRunDto[];
  return body.map(mapReindexRun);
}

async function listAssetPage(path: string, cursor?: string): Promise<AssetTimelinePage> {
  const params = new URLSearchParams({ limit: "60" });
  if (cursor !== undefined) {
    params.set("cursor", cursor);
  }
  const response = await request(`${path}?${params.toString()}`, { method: "GET" });
  return mapAssetPage((await response.json()) as AssetTimelinePageDto);
}

async function createUpload(input: {
  originalFilename: string;
  expectedSize: number;
  expectedBlake3: string;
  mediaType: string;
  clientUploadKey: string;
}): Promise<UploadSession> {
  const response = await request("/uploads", {
    method: "POST",
    headers: csrfHeaders(),
    body: jsonBody({
      original_filename: input.originalFilename,
      expected_size: input.expectedSize,
      expected_blake3: input.expectedBlake3,
      media_type: input.mediaType,
      client_upload_key: input.clientUploadKey
    })
  });
  return mapUpload((await response.json()) as UploadSessionDto);
}

async function putUploadPart(uploadId: string, partIndex: number, bytes: ArrayBuffer): Promise<void> {
  await request(`/uploads/${uploadId}/parts/${String(partIndex)}`, {
    method: "PUT",
    headers: csrfHeaders(),
    body: bytes
  });
}

async function completeUpload(uploadId: string): Promise<CompleteUploadResult> {
  const response = await request(`/uploads/${uploadId}/complete`, {
    method: "POST",
    headers: csrfHeaders()
  });
  const body = (await response.json()) as CompleteUploadDto;
  return {
    upload: mapUpload(body.upload),
    promoted: {
      assetId: body.promoted.asset_id
    }
  };
}

async function hashFileBlake3(file: File, onProgress: (loadedBytes: number) => void): Promise<string> {
  const hasher = await createBLAKE3();
  hasher.init();
  for (let offset = 0; offset < file.size; offset += uploadPartSizeBytes) {
    const end = Math.min(offset + uploadPartSizeBytes, file.size);
    const bytes = new Uint8Array(await file.slice(offset, end).arrayBuffer());
    hasher.update(bytes);
    onProgress(end);
  }
  return hasher.digest("hex");
}

function mapSession(session: SessionDto): Session {
  return {
    sessionId: session.session_id,
    deviceName: session.device_name,
    userAgent: session.user_agent,
    createdAt: session.created_at,
    lastSeenAt: session.last_seen_at,
    expiresAt: session.expires_at,
    isCurrent: session.is_current
  };
}

function mapAssetPage(body: AssetTimelinePageDto): AssetTimelinePage {
  return {
    items: body.items.map((item) => ({
      assetId: item.asset_id,
      createdAt: item.created_at,
      trashedAt: item.trashed_at,
      favoriteAt: item.favorite_at,
      originalBlake3: item.original_blake3,
      mediaType: item.media_type,
      sizeBytes: item.size_bytes,
      originalFilename: item.original_filename,
      thumbnail: item.thumbnail,
      preview: item.preview
    })),
    nextCursor: body.next_cursor
  };
}

function mapUpload(upload: UploadSessionDto): UploadSession {
  return {
    uploadId: upload.upload_id,
    status: upload.status,
    expectedSize: upload.expected_size,
    expectedBlake3: upload.expected_blake3,
    mediaType: upload.media_type,
    committedParts: upload.committed_parts
  };
}

function mapModelPack(pack: ModelPackSummaryDto): ModelPackSummary {
  return {
    modelPackId: pack.model_pack_id,
    kind: pack.kind,
    runtime: pack.runtime,
    modelKey: pack.model_key,
    modelRevision: pack.model_revision,
    status: pack.status,
    selfTestStatus: pack.self_test_status,
    embeddingDimension: pack.embedding_dimension,
    distanceMetric: pack.distance_metric,
    updatedAt: pack.updated_at
  };
}

function mapReindexRun(run: ModelReindexRunDto): ModelReindexRun {
  return {
    reindexRunId: run.reindex_run_id,
    modelPackId: run.model_pack_id,
    status: run.status,
    totalAssets: run.total_assets,
    queuedAssets: run.queued_assets,
    processedAssets: run.processed_assets,
    failedAssets: run.failed_assets
  };
}

function jsonBody(value: unknown): string {
  return JSON.stringify(value);
}

function secondFactorBody(input: SecondFactorInput) {
  return {
    password: input.password,
    totp_code: blankToNull(input.totpCode),
    recovery_code: blankToNull(input.recoveryCode)
  };
}

function blankToNull(value: string | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed === undefined || trimmed.length === 0 ? null : trimmed;
}

function csrfHeaders(): Headers {
  const headers = new Headers();
  const csrfToken = document.cookie
    .split("; ")
    .find((cookie) => cookie.startsWith("mirror_csrf="))
    ?.slice("mirror_csrf=".length);

  if (csrfToken !== undefined) {
    headers.set("x-csrf-token", csrfToken);
  }

  return headers;
}

async function request(path: string, init: RequestInit): Promise<Response> {
  const headers = new Headers(init.headers);
  if (init.body !== undefined && typeof init.body === "string" && !headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }

  const response = await fetch(path, {
    ...init,
    credentials: "include",
    headers
  });

  if (!response.ok) {
    throw await parseApiError(response);
  }

  return response;
}

async function parseApiError(response: Response): Promise<ApiError> {
  try {
    const body = (await response.json()) as ApiErrorBody;
    return new ApiError(response.status, body);
  } catch {
    return new ApiError(response.status, {
      error: "internal_error",
      message: "request failed"
    });
  }
}

function makeClientUploadKey(): string {
  const browserCrypto = globalThis.crypto as Crypto & { randomUUID?: () => string };
  if (typeof browserCrypto.randomUUID === "function") {
    return browserCrypto.randomUUID();
  }

  const random = browserCrypto.getRandomValues(new Uint8Array(16));
  random[6] = (random[6] & 0x0f) | 0x40;
  random[8] = (random[8] & 0x3f) | 0x80;
  const hex = Array.from(random, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
