export type ApiErrorCode =
  | "csrf_invalid"
  | "csrf_required"
  | "database_unavailable"
  | "invalid_credentials"
  | "invalid_display_name"
  | "invalid_password"
  | "invalid_asset_list"
  | "invalid_asset_request"
  | "invalid_setup_token"
  | "asset_not_found"
  | "owner_exists"
  | "setup_unavailable"
  | "internal_error";

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
    body: JSON.stringify({
      setup_token: input.setupToken,
      display_name: input.displayName,
      password: input.password
    })
  });
}

export type LoginInput = {
  password: string;
  deviceName?: string;
};

export async function login(input: LoginInput): Promise<void> {
  await request("/auth/login", {
    method: "POST",
    body: JSON.stringify({
      password: input.password,
      device_name: input.deviceName ?? null
    })
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
  const params = new URLSearchParams({ limit: "60" });
  if (cursor !== undefined) {
    params.set("cursor", cursor);
  }
  const response = await request(`/assets?${params.toString()}`, { method: "GET" });
  const body = (await response.json()) as AssetTimelinePageDto;

  return {
    items: body.items.map((item) => ({
      assetId: item.asset_id,
      createdAt: item.created_at,
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

export function derivativeUrl(assetId: string, kind: "thumbnail" | "preview"): string {
  return `/assets/${assetId}/derivatives/${kind}`;
}

async function request(path: string, init: RequestInit): Promise<Response> {
  const headers = new Headers(init.headers);
  if (init.body !== undefined) {
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
