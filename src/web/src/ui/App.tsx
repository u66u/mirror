import { type VirtualItem, useVirtualizer } from "@tanstack/react-virtual";
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight, LogOut, Play, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  ApiError,
  type AssetTimelineItem,
  derivativeUrl,
  listAssets,
  listSessions,
  login,
  logout,
  setupOwner
} from "../api/client";

type Mode = "setup" | "login" | "timeline";
type TimelineView = "assets" | "sessions";

export function App() {
  const [mode, setMode] = useState<Mode>("setup");
  const queryClient = useQueryClient();

  if (mode === "timeline") {
    return (
      <Timeline
        onLogout={() => {
          queryClient.removeQueries();
          setMode("login");
        }}
      />
    );
  }

  return (
    <main className="app-shell">
      <section className="auth-panel" aria-label="Mirror access">
        <div className="brand-row">
          <div>
            <h1>Mirror</h1>
            <p>Private photo vault</p>
          </div>
          <ModeSwitch mode={mode} onModeChange={setMode} />
        </div>
        {mode === "setup" ? (
          <SetupForm
            onDone={() => {
              setMode("login");
            }}
          />
        ) : (
          <LoginForm
            onDone={() => {
              setMode("timeline");
            }}
          />
        )}
      </section>
    </main>
  );
}

function ModeSwitch(props: { mode: Mode; onModeChange: (mode: Mode) => void }) {
  return (
    <div className="segmented" role="tablist" aria-label="Access mode">
      <button
        className={props.mode === "setup" ? "active" : ""}
        type="button"
        onClick={() => {
          props.onModeChange("setup");
        }}
      >
        Setup
      </button>
      <button
        className={props.mode === "login" ? "active" : ""}
        type="button"
        onClick={() => {
          props.onModeChange("login");
        }}
      >
        Login
      </button>
    </div>
  );
}

function SetupForm(props: { onDone: () => void }) {
  const [setupToken, setSetupToken] = useState("");
  const [displayName, setDisplayName] = useState("Owner");
  const [password, setPassword] = useState("");
  const mutation = useMutation({
    mutationFn: setupOwner,
    onSuccess: props.onDone
  });

  return (
    <form
      className="form-stack"
      onSubmit={(event) => {
        event.preventDefault();
        mutation.mutate({ setupToken, displayName, password });
      }}
    >
      <Field label="Setup token">
        <input
          autoComplete="one-time-code"
          value={setupToken}
          onChange={(event) => {
            setSetupToken(event.target.value);
          }}
          required
        />
      </Field>
      <Field label="Display name">
        <input
          autoComplete="name"
          value={displayName}
          onChange={(event) => {
            setDisplayName(event.target.value);
          }}
          required
        />
      </Field>
      <Field label="Password">
        <input
          autoComplete="new-password"
          minLength={12}
          type="password"
          value={password}
          onChange={(event) => {
            setPassword(event.target.value);
          }}
          required
        />
      </Field>
      <SubmitButton pending={mutation.isPending}>Create owner</SubmitButton>
      <FormError error={mutation.error} />
    </form>
  );
}

function LoginForm(props: { onDone: () => void }) {
  const [password, setPassword] = useState("");
  const mutation = useMutation({
    mutationFn: login,
    onSuccess: props.onDone
  });

  return (
    <form
      className="form-stack"
      onSubmit={(event) => {
        event.preventDefault();
        mutation.mutate({ password, deviceName: "Web browser" });
      }}
    >
      <Field label="Password">
        <input
          autoComplete="current-password"
          type="password"
          value={password}
          onChange={(event) => {
            setPassword(event.target.value);
          }}
          required
        />
      </Field>
      <SubmitButton pending={mutation.isPending}>Login</SubmitButton>
      {mutation.isSuccess ? <p className="status-ok">Signed in</p> : null}
      <FormError error={mutation.error} />
    </form>
  );
}

function Timeline(props: { onLogout: () => void }) {
  const [view, setView] = useState<TimelineView>("assets");
  const [selectedAssetId, setSelectedAssetId] = useState<string | null>(null);
  const assetsQuery = useInfiniteQuery({
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => listAssets(pageParam),
    queryKey: ["assets"],
    getNextPageParam: (lastPage) => lastPage.nextCursor ?? undefined
  });
  const assets = useMemo(
    () => assetsQuery.data?.pages.flatMap((page) => page.items) ?? [],
    [assetsQuery.data]
  );
  const selectedIndex = assets.findIndex((asset) => asset.assetId === selectedAssetId);
  const sessionsQuery = useQuery({
    enabled: view === "sessions",
    queryFn: listSessions,
    queryKey: ["sessions"]
  });
  const logoutMutation = useMutation({
    mutationFn: logout,
    onSuccess: props.onLogout
  });

  return (
    <main className="timeline-shell">
      <header className="timeline-header">
        <div>
          <h1>Mirror</h1>
          <p>{assets.length} loaded</p>
        </div>
        <div className="timeline-actions">
          <div className="segmented view-tabs" role="tablist" aria-label="Vault view">
            <button
              aria-selected={view === "assets"}
              className={view === "assets" ? "active" : ""}
              role="tab"
              type="button"
              onClick={() => {
                setView("assets");
              }}
            >
              Photos
            </button>
            <button
              aria-selected={view === "sessions"}
              className={view === "sessions" ? "active" : ""}
              role="tab"
              type="button"
              onClick={() => {
                setView("sessions");
              }}
            >
              Sessions
            </button>
          </div>
          <button
            className="secondary"
            disabled={logoutMutation.isPending}
            type="button"
            onClick={() => {
              logoutMutation.mutate();
            }}
          >
            <LogOut aria-hidden="true" size={16} strokeWidth={1.8} />
            {logoutMutation.isPending ? "Logging out..." : "Log out"}
          </button>
        </div>
      </header>
      {logoutMutation.error ? <FormError error={logoutMutation.error} /> : null}
      {view === "assets" ? (
        <>
          {assetsQuery.isPending ? <p className="muted">Loading</p> : null}
          {assetsQuery.error ? <FormError error={assetsQuery.error} /> : null}
          {assets.length === 0 && !assetsQuery.isPending ? (
            <p className="muted">No assets yet</p>
          ) : null}
          {assets.length > 0 ? (
            <AssetGrid
              assets={assets}
              hasNextPage={assetsQuery.hasNextPage}
              isFetchingNextPage={assetsQuery.isFetchingNextPage}
              onLoadMore={() => {
                void assetsQuery.fetchNextPage();
              }}
              onOpen={setSelectedAssetId}
            />
          ) : null}
        </>
      ) : (
        <section className="session-panel" aria-labelledby="sessions-heading">
          <div className="session-heading">
            <div>
              <h2 id="sessions-heading">Active sessions</h2>
              <p>Browsers currently signed in to Mirror.</p>
            </div>
          </div>
          {sessionsQuery.isPending ? <p className="muted session-status">Loading sessions</p> : null}
          {sessionsQuery.error ? <FormError error={sessionsQuery.error} /> : null}
          {sessionsQuery.data?.length === 0 ? (
            <p className="muted session-status">No active sessions</p>
          ) : null}
          {sessionsQuery.data ? (
            <ul className="session-list">
              {sessionsQuery.data.map((session) => (
                <li className="session-row" key={session.sessionId}>
                  <div className="session-title">
                    <div>
                      <h3>{session.deviceName ?? "Web browser"}</h3>
                      <p>{session.userAgent ?? "Browser details unavailable"}</p>
                    </div>
                    {session.isCurrent ? <span className="current-badge">Current</span> : null}
                  </div>
                  <dl className="session-meta">
                    <div>
                      <dt>Last active</dt>
                      <dd>
                        <time dateTime={session.lastSeenAt ?? session.createdAt}>
                          {formatDate(session.lastSeenAt ?? session.createdAt)}
                        </time>
                      </dd>
                    </div>
                    <div>
                      <dt>Signed in</dt>
                      <dd>
                        <time dateTime={session.createdAt}>{formatDate(session.createdAt)}</time>
                      </dd>
                    </div>
                    <div>
                      <dt>Expires</dt>
                      <dd>
                        <time dateTime={session.expiresAt}>{formatDate(session.expiresAt)}</time>
                      </dd>
                    </div>
                  </dl>
                </li>
              ))}
            </ul>
          ) : null}
        </section>
      )}
      {selectedIndex >= 0 ? (
        <AssetViewer
          asset={assets[selectedIndex]}
          canGoNext={selectedIndex < assets.length - 1}
          canGoPrevious={selectedIndex > 0}
          onClose={() => {
            setSelectedAssetId(null);
          }}
          onNext={() => {
            setSelectedAssetId(assets[selectedIndex + 1]?.assetId ?? null);
          }}
          onPrevious={() => {
            setSelectedAssetId(assets[selectedIndex - 1]?.assetId ?? null);
          }}
        />
      ) : null}
    </main>
  );
}

function AssetGrid(props: {
  assets: AssetTimelineItem[];
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  onLoadMore: () => void;
  onOpen: (assetId: string) => void;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [containerWidth, setContainerWidth] = useState(960);
  const gap = 10;
  const columns = Math.max(1, Math.floor((containerWidth + gap) / (148 + gap)));
  const tileWidth = Math.max(104, (containerWidth - gap * (columns - 1)) / columns);
  const rowHeight = tileWidth + 48;
  const rowCount = Math.ceil(props.assets.length / columns);
  // TanStack Virtual is intentionally imperative; React Compiler must not memoize this hook.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rowCount,
    estimateSize: () => rowHeight,
    gap,
    getScrollElement: () => scrollRef.current,
    getItemKey: (index) => props.assets[index * columns]?.assetId ?? index,
    initialRect: { width: 960, height: 640 },
    overscan: 3
  });

  useEffect(() => {
    const element = scrollRef.current;
    if (element === null) {
      return;
    }
    const updateWidth = () => {
      setContainerWidth(element.clientWidth || 960);
    };
    updateWidth();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", updateWidth);
      return () => {
        window.removeEventListener("resize", updateWidth);
      };
    }
    const observer = new ResizeObserver(updateWidth);
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);

  useEffect(() => {
    virtualizer.measure();
  }, [columns, rowHeight, virtualizer]);
  const measuredRows = virtualizer.getVirtualItems();
  const fallbackRows: VirtualItem[] =
    measuredRows.length === 0
      ? Array.from({ length: Math.min(rowCount, 20) }, (_, index) => ({
          end: (index + 1) * rowHeight + index * gap,
          index,
          key: `fallback-${String(index)}`,
          lane: 0,
          size: rowHeight,
          start: index * (rowHeight + gap)
        }))
      : [];
  const visibleRows = measuredRows.length > 0 ? measuredRows : fallbackRows;
  const fallbackHeight =
    fallbackRows.length === 0
      ? 0
      : fallbackRows[fallbackRows.length - 1]?.end ?? 0;

  return (
    <section aria-label="Asset timeline" className="asset-timeline">
      <div
        className="asset-scroll"
        ref={scrollRef}
        onScroll={(event) => {
          const element = event.currentTarget;
          const nearEnd =
            element.scrollHeight - element.scrollTop - element.clientHeight < rowHeight * 2;
          if (nearEnd && props.hasNextPage && !props.isFetchingNextPage) {
            props.onLoadMore();
          }
        }}
      >
        <div
          className="virtual-grid"
          style={{
            height: Math.max(virtualizer.getTotalSize(), fallbackHeight)
          }}
        >
          {visibleRows.map((virtualRow) => {
            const firstIndex = virtualRow.index * columns;
            const rowAssets = props.assets.slice(firstIndex, firstIndex + columns);
            return (
              <div
                className="virtual-row"
                data-index={virtualRow.index}
                key={virtualRow.key}
                style={{
                  gridTemplateColumns: `repeat(${String(columns)}, minmax(0, 1fr))`,
                  height: virtualRow.size,
                  transform: `translateY(${String(virtualRow.start)}px)`
                }}
              >
                {rowAssets.map((asset) => (
                  <AssetTile asset={asset} key={asset.assetId} onOpen={props.onOpen} />
                ))}
              </div>
            );
          })}
        </div>
        {props.hasNextPage ? (
          <button
            className="load-more"
            disabled={props.isFetchingNextPage}
            type="button"
            onClick={props.onLoadMore}
          >
            {props.isFetchingNextPage ? "Loading..." : "Load more"}
          </button>
        ) : null}
      </div>
    </section>
  );
}

function AssetTile(props: {
  asset: AssetTimelineItem;
  onOpen: (assetId: string) => void;
}) {
  const label = props.asset.originalFilename ?? props.asset.assetId;
  return (
    <button
      aria-label={`Open ${label}`}
      className="asset-tile"
      type="button"
      onClick={() => {
        props.onOpen(props.asset.assetId);
      }}
    >
      <div className="asset-media">
        {props.asset.thumbnail === null ? (
          <div className="asset-placeholder">{props.asset.mediaType}</div>
        ) : (
          <img
            alt={label}
            height={props.asset.thumbnail.height}
            loading="lazy"
            src={derivativeUrl(props.asset.assetId, "thumbnail")}
            width={props.asset.thumbnail.width}
          />
        )}
        {props.asset.mediaType.startsWith("video/") ? (
          <span className="video-badge" title="Video">
            <Play aria-hidden="true" fill="currentColor" size={16} />
          </span>
        ) : null}
      </div>
      <span className="asset-meta">
        <span>{label}</span>
        <time dateTime={props.asset.createdAt}>{formatDate(props.asset.createdAt, false)}</time>
      </span>
    </button>
  );
}

function AssetViewer(props: {
  asset: AssetTimelineItem;
  canGoNext: boolean;
  canGoPrevious: boolean;
  onClose: () => void;
  onNext: () => void;
  onPrevious: () => void;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const label = props.asset.originalFilename ?? props.asset.assetId;

  useEffect(() => {
    dialogRef.current?.focus();
  }, [props.asset.assetId]);

  return (
    <div
      aria-label={label}
      aria-modal="true"
      className="asset-viewer"
      ref={dialogRef}
      role="dialog"
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          props.onClose();
        } else if (event.key === "ArrowLeft" && props.canGoPrevious) {
          props.onPrevious();
        } else if (event.key === "ArrowRight" && props.canGoNext) {
          props.onNext();
        }
      }}
    >
      <button
        aria-label="Close preview"
        className="viewer-icon viewer-close"
        type="button"
        onClick={props.onClose}
      >
        <X aria-hidden="true" />
      </button>
      <button
        aria-label="Previous asset"
        className="viewer-icon viewer-previous"
        disabled={!props.canGoPrevious}
        type="button"
        onClick={props.onPrevious}
      >
        <ChevronLeft aria-hidden="true" />
      </button>
      <div className="viewer-media">
        {props.asset.preview === null ? (
          <div className="viewer-placeholder">{props.asset.mediaType}</div>
        ) : (
          <img
            alt={`${label} preview`}
            height={props.asset.preview.height}
            src={derivativeUrl(props.asset.assetId, "preview")}
            width={props.asset.preview.width}
          />
        )}
      </div>
      <button
        aria-label="Next asset"
        className="viewer-icon viewer-next"
        disabled={!props.canGoNext}
        type="button"
        onClick={props.onNext}
      >
        <ChevronRight aria-hidden="true" />
      </button>
      <footer className="viewer-footer">
        <strong>{label}</strong>
        <span>
          {formatDate(props.asset.createdAt)} · {formatBytes(props.asset.sizeBytes)}
        </span>
      </footer>
    </div>
  );
}

function formatBytes(value: number): string {
  if (value < 1024) {
    return `${String(value)} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let amount = value / 1024;
  let unit = units[0];
  for (const candidate of units) {
    unit = candidate;
    if (amount < 1024 || candidate === units[units.length - 1]) {
      break;
    }
    amount /= 1024;
  }
  return `${amount.toFixed(amount >= 10 ? 0 : 1)} ${unit}`;
}

function formatDate(value: string, includeTime = true): string {
  return new Intl.DateTimeFormat(
    undefined,
    includeTime
      ? {
          dateStyle: "medium",
          timeStyle: "short"
        }
      : {
          day: "2-digit",
          month: "short",
          year: "numeric"
        }
  ).format(new Date(value));
}

function Field(props: { label: string; children: React.ReactNode }) {
  return (
    <label className="field">
      <span>{props.label}</span>
      {props.children}
    </label>
  );
}

function SubmitButton(props: { children: React.ReactNode; pending: boolean }) {
  return (
    <button className="primary" disabled={props.pending} type="submit">
      {props.pending ? "Working..." : props.children}
    </button>
  );
}

function FormError(props: { error: Error | null }) {
  if (props.error === null) {
    return null;
  }

  const message = props.error instanceof ApiError ? props.error.message : "Request failed";
  return <p className="status-error">{message}</p>;
}
