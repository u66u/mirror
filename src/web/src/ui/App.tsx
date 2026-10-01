import { type VirtualItem, useVirtualizer } from "@tanstack/react-virtual";
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Archive,
  Brain,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  Download,
  FileDown,
  Heart,
  ImageIcon,
  KeyRound,
  Layers3,
  Loader2,
  LockKeyhole,
  LogOut,
  Play,
  RefreshCcw,
  Search,
  Server,
  ShieldCheck,
  Sparkles,
  Trash2,
  UploadCloud,
  Users,
  X
} from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  ApiError,
  type AssetTimelineItem,
  type FaceAlbumItem,
  type ModelPackSummary,
  type Session,
  type UploadProgress,
  activateModelPack,
  createDeviceToken,
  derivativeUrl,
  trashedDerivativeUrl,
  disableTotp,
  enableTotp,
  favoriteAsset,
  faceChipUrl,
  getExportManifest,
  getMfaStatus,
  getReady,
  installModelPack,
  listAssets,
  listModelPacks,
  listPeople,
  listSessions,
  listTrashedAssets,
  listUnassignedFaces,
  login,
  logout,
  purgeAsset,
  restoreAsset,
  rotateRecoveryCodes,
  runModelPackSelfTest,
  searchAssets,
  setupOwner,
  setupTotp,
  startModelReindex,
  trashAsset,
  unfavoriteAsset,
  uploadAsset
} from "../api/client";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Input } from "../components/ui/input";
import { Textarea } from "../components/ui/textarea";
import { cn } from "../lib/utils";

type Mode = "setup" | "login" | "dashboard";
type AdminView =
  | "photos"
  | "upload"
  | "search"
  | "people"
  | "models"
  | "exports"
  | "trash"
  | "sessions"
  | "security"
  | "system";

export function App() {
  const [mode, setMode] = useState<Mode>("setup");
  const queryClient = useQueryClient();

  if (mode === "dashboard") {
    return (
      <Dashboard
        onLogout={() => {
          queryClient.removeQueries();
          setMode("login");
        }}
      />
    );
  }

  return (
    <main className="min-h-dvh bg-[var(--background)] p-4 text-[var(--foreground)] sm:p-6">
      <section
        className="mx-auto grid min-h-[calc(100dvh-2rem)] max-w-6xl items-center gap-8 lg:grid-cols-[1.05fr_0.95fr]"
        aria-label="Mirror access"
      >
        <div aria-hidden="true" className="hidden lg:block">
          <Badge variant="outline" className="mb-5 bg-[var(--surface)]">
            Owner console
          </Badge>
          <h1 className="max-w-xl text-5xl font-semibold tracking-[-0.045em]">
            Mirror
          </h1>
          <p className="mt-5 max-w-xl text-lg leading-8 text-[var(--muted-foreground)]">
            Private photo and video vault administration for uploads, sessions, exports,
            model packs, people review, and storage health.
          </p>
          <div className="mt-8 grid max-w-2xl grid-cols-3 gap-3">
            <MiniFeature icon={<ImageIcon />} label="Timeline" />
            <MiniFeature icon={<ShieldCheck />} label="Security" />
            <MiniFeature icon={<Brain />} label="Models" />
          </div>
        </div>
        <Card className="mx-auto w-full max-w-md">
          <CardHeader className="gap-4">
            <div className="flex items-start justify-between gap-4">
              <div>
                <h1 className="text-3xl font-semibold tracking-[-0.035em]">Mirror</h1>
                <CardDescription>Private photo vault</CardDescription>
              </div>
              <ModeSwitch mode={mode} onModeChange={setMode} />
            </div>
          </CardHeader>
          <CardContent>
            {mode === "setup" ? (
              <SetupForm
                onDone={() => {
                  setMode("login");
                }}
              />
            ) : (
              <LoginForm
                onDone={() => {
                  setMode("dashboard");
                }}
              />
            )}
          </CardContent>
        </Card>
      </section>
    </main>
  );
}

function MiniFeature(props: { icon: ReactNode; label: string }) {
  return (
    <div className="rounded-2xl border border-[var(--border)] bg-[var(--surface)] p-4 text-sm text-[var(--muted-foreground)]">
      <span className="mb-3 flex size-9 items-center justify-center rounded-xl bg-[var(--primary-soft)] text-[var(--primary-text)]">
        {props.icon}
      </span>
      {props.label}
    </div>
  );
}

function ModeSwitch(props: { mode: Mode; onModeChange: (mode: Mode) => void }) {
  return (
    <div
      className="inline-flex rounded-xl border border-[var(--border)] bg-[var(--surface-muted)] p-1"
      role="tablist"
      aria-label="Access mode"
    >
      {(["setup", "login"] as const).map((mode) => (
        <button
          aria-selected={props.mode === mode}
          className={cn(
            "rounded-lg px-3 py-1.5 text-sm font-medium capitalize text-[var(--muted-foreground)] transition-colors",
            props.mode === mode && "bg-[var(--surface)] text-[var(--foreground)] shadow-sm"
          )}
          key={mode}
          type="button"
          onClick={() => {
            props.onModeChange(mode);
          }}
        >
          {mode === "setup" ? "Setup" : "Login"}
        </button>
      ))}
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
      className="grid gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        mutation.mutate({ setupToken, displayName, password });
      }}
    >
      <Field label="Setup token">
        <Input
          autoComplete="one-time-code"
          value={setupToken}
          onChange={(event) => {
            setSetupToken(event.target.value);
          }}
          required
        />
      </Field>
      <Field label="Display name">
        <Input
          autoComplete="name"
          value={displayName}
          onChange={(event) => {
            setDisplayName(event.target.value);
          }}
          required
        />
      </Field>
      <Field label="Password">
        <Input
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
      <Button disabled={mutation.isPending} type="submit" size="lg">
        {mutation.isPending ? <Loader2 className="animate-spin" aria-hidden="true" /> : null}
        Create owner
      </Button>
      <FormError error={mutation.error} />
    </form>
  );
}

function LoginForm(props: { onDone: () => void }) {
  const [password, setPassword] = useState("");
  const [secondFactor, setSecondFactor] = useState("");
  const mutation = useMutation({
    mutationFn: login,
    onSuccess: props.onDone
  });

  return (
    <form
      className="grid gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        mutation.mutate({
          password,
          deviceName: "Web browser",
          totpCode: secondFactor
        });
      }}
    >
      <Field label="Password">
        <Input
          autoComplete="current-password"
          type="password"
          value={password}
          onChange={(event) => {
            setPassword(event.target.value);
          }}
          required
        />
      </Field>
      <Field label="Second factor">
        <Input
          autoComplete="one-time-code"
          inputMode="numeric"
          placeholder="Optional TOTP code"
          value={secondFactor}
          onChange={(event) => {
            setSecondFactor(event.target.value);
          }}
        />
      </Field>
      <Button disabled={mutation.isPending} type="submit" size="lg">
        {mutation.isPending ? <Loader2 className="animate-spin" aria-hidden="true" /> : null}
        Login
      </Button>
      {mutation.isSuccess ? <p className="text-sm text-[var(--success)]">Signed in</p> : null}
      <FormError error={mutation.error} />
    </form>
  );
}

function Dashboard(props: { onLogout: () => void }) {
  const [view, setView] = useState<AdminView>("photos");
  const [selectedAssetId, setSelectedAssetId] = useState<string | null>(null);
  const queryClient = useQueryClient();
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
  const logoutMutation = useMutation({
    mutationFn: logout,
    onSuccess: props.onLogout
  });

  const title = viewTitles[view];

  return (
    <main className="flex h-dvh overflow-hidden bg-[var(--background)] text-[var(--foreground)]">
      <aside className="hidden w-72 shrink-0 border-r border-[var(--border)] bg-[var(--surface)] p-4 lg:flex lg:flex-col">
        <div className="mb-6 rounded-2xl border border-[var(--border)] bg-[var(--surface-muted)] p-4">
          <div className="flex items-center gap-3">
            <div className="flex size-10 items-center justify-center rounded-2xl bg-[var(--primary)] text-[var(--primary-foreground)]">
              <Layers3 aria-hidden="true" />
            </div>
            <div>
              <h1 className="text-lg font-semibold tracking-[-0.025em]">Mirror</h1>
              <p className="text-xs text-[var(--muted-foreground)]">Admin dashboard</p>
            </div>
          </div>
        </div>
        <AdminNav view={view} onViewChange={setView} />
        <div className="mt-auto rounded-2xl border border-[var(--border)] bg-[var(--surface-muted)] p-4">
          <p className="text-xs font-medium uppercase tracking-[0.18em] text-[var(--muted-foreground)]">
            Library
          </p>
          <p className="mt-2 text-2xl font-semibold tracking-[-0.035em]">{assets.length} loaded</p>
          <p className="mt-1 text-sm text-[var(--muted-foreground)]">
            {formatBytes(sumBytes(assets))} indexed in this view
          </p>
        </div>
      </aside>
      <section className="flex min-w-0 flex-1 flex-col">
        <header className="border-b border-[var(--border)] bg-[color-mix(in_srgb,var(--background)_88%,transparent)] px-4 py-3 backdrop-blur sm:px-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <p className="text-xs font-medium uppercase tracking-[0.2em] text-[var(--muted-foreground)]">
                Mirror
              </p>
              <h2 className="mt-1 text-2xl font-semibold tracking-[-0.035em]">{title}</h2>
              <p className="mt-1 text-sm text-[var(--muted-foreground)]">{assets.length} loaded</p>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                type="button"
                onClick={() => {
                  void queryClient.invalidateQueries();
                }}
              >
                <RefreshCcw aria-hidden="true" />
                Refresh
              </Button>
              <Button
                variant="outline"
                disabled={logoutMutation.isPending}
                type="button"
                onClick={() => {
                  logoutMutation.mutate();
                }}
              >
                <LogOut aria-hidden="true" />
                {logoutMutation.isPending ? "Logging out..." : "Log out"}
              </Button>
            </div>
          </div>
          <div className="mt-4 lg:hidden">
            <AdminNav compact view={view} onViewChange={setView} />
          </div>
          {logoutMutation.error ? <FormError error={logoutMutation.error} /> : null}
        </header>
        <div className="min-h-0 flex-1 overflow-hidden p-4 sm:p-6">
          {view === "photos" ? (
            <PhotosPanel
              assets={assets}
              assetsQuery={assetsQuery}
              onOpen={setSelectedAssetId}
            />
          ) : null}
          {view === "upload" ? (
            <UploadPanel
              onDone={() => {
                setView("photos");
              }}
            />
          ) : null}
          {view === "search" ? <SearchPanel onOpen={setSelectedAssetId} /> : null}
          {view === "people" ? <PeoplePanel /> : null}
          {view === "models" ? <ModelsPanel /> : null}
          {view === "exports" ? <ExportsPanel /> : null}
          {view === "trash" ? <TrashPanel /> : null}
          {view === "sessions" ? <SessionsPanel active /> : null}
          {view === "security" ? <SecurityPanel /> : null}
          {view === "system" ? <SystemPanel /> : null}
        </div>
      </section>
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

function AdminNav(props: {
  compact?: boolean;
  view: AdminView;
  onViewChange: (view: AdminView) => void;
}) {
  const items: Array<{ view: AdminView; label: string; icon: ReactNode }> = [
    { view: "photos", label: "Photos", icon: <ImageIcon /> },
    { view: "upload", label: "Upload", icon: <UploadCloud /> },
    { view: "search", label: "Search", icon: <Search /> },
    { view: "people", label: "People", icon: <Users /> },
    { view: "models", label: "Models", icon: <Brain /> },
    { view: "exports", label: "Exports", icon: <FileDown /> },
    { view: "trash", label: "Trash", icon: <Trash2 /> },
    { view: "sessions", label: "Sessions", icon: <ShieldCheck /> },
    { view: "security", label: "Security", icon: <KeyRound /> },
    { view: "system", label: "System", icon: <Server /> }
  ];

  return (
    <nav
      className={cn(
        props.compact
          ? "flex gap-2 overflow-x-auto pb-1"
          : "grid gap-1"
      )}
      role={props.compact ? undefined : "tablist"}
      aria-label="Admin sections"
    >
      {items.map((item) => (
        <button
          aria-current={props.compact && props.view === item.view ? "page" : undefined}
          aria-selected={props.compact ? undefined : props.view === item.view}
          className={cn(
            "inline-flex items-center gap-3 rounded-xl px-3 py-2.5 text-left text-sm font-medium text-[var(--muted-foreground)] transition-colors",
            props.compact && "shrink-0 border border-[var(--border)] bg-[var(--surface)]",
            props.view === item.view &&
              "bg-[var(--primary-soft)] text-[var(--primary-text)]"
          )}
          key={item.view}
          role={props.compact ? undefined : "tab"}
          type="button"
          onClick={() => {
            props.onViewChange(item.view);
          }}
        >
          <span className="flex size-5 items-center justify-center [&_svg]:size-4">
            {item.icon}
          </span>
          {item.label}
        </button>
      ))}
    </nav>
  );
}

function PhotosPanel(props: {
  assets: AssetTimelineItem[];
  assetsQuery: ReturnType<typeof useInfiniteQuery<Awaited<ReturnType<typeof listAssets>>, Error>>;
  onOpen: (assetId: string) => void;
}) {
  return (
    <section className="flex h-full min-h-0 flex-col gap-4" aria-label="Asset timeline">
      <div className="grid gap-3 md:grid-cols-4">
        <MetricCard label="Loaded assets" value={String(props.assets.length)} icon={<ImageIcon />} />
        <MetricCard label="Stored size" value={formatBytes(sumBytes(props.assets))} icon={<Archive />} />
        <MetricCard label="Favorites" value={String(props.assets.filter((asset) => asset.favoriteAt !== null).length)} icon={<Heart />} />
        <MetricCard label="Videos" value={String(props.assets.filter((asset) => asset.mediaType.startsWith("video/")).length)} icon={<Play />} />
      </div>
      {props.assetsQuery.isPending ? <p className="text-sm text-[var(--muted-foreground)]">Loading</p> : null}
      {props.assetsQuery.error ? <FormError error={props.assetsQuery.error} /> : null}
      {props.assets.length === 0 && !props.assetsQuery.isPending ? (
        <EmptyState icon={<ImageIcon />} title="No assets yet" description="Upload photos or videos to start building the vault." />
      ) : null}
      {props.assets.length > 0 ? (
        <AssetGrid
          assets={props.assets}
          hasNextPage={props.assetsQuery.hasNextPage}
          isFetchingNextPage={props.assetsQuery.isFetchingNextPage}
          onLoadMore={() => {
            void props.assetsQuery.fetchNextPage();
          }}
          onOpen={props.onOpen}
        />
      ) : null}
    </section>
  );
}

function UploadPanel(props: { onDone: () => void }) {
  const [file, setFile] = useState<File | null>(null);
  const [progress, setProgress] = useState<UploadProgress | null>(null);
  const queryClient = useQueryClient();
  const mutation = useMutation({
    mutationFn: async (selected: File) => uploadAsset(selected, setProgress),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["assets"] });
      props.onDone();
    }
  });

  const percent =
    progress === null || progress.totalBytes === 0
      ? 0
      : Math.round((progress.loadedBytes / progress.totalBytes) * 100);

  return (
    <PanelGrid>
      <Card className="lg:col-span-2">
        <CardHeader>
          <CardTitle>Upload originals</CardTitle>
          <CardDescription>
            Mirror hashes the file in the browser with BLAKE3, uploads in 4 MiB parts,
            then promotes the asset.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form
            className="grid gap-4"
            onSubmit={(event) => {
              event.preventDefault();
              if (file !== null) {
                mutation.mutate(file);
              }
            }}
          >
            <label className="grid cursor-pointer place-items-center rounded-2xl border border-dashed border-[var(--border-strong)] bg-[var(--surface-muted)] p-10 text-center">
              <UploadCloud className="mb-4 size-9 text-[var(--primary-text)]" aria-hidden="true" />
              <span className="text-base font-medium">
                {file === null ? "Choose a photo or video" : file.name}
              </span>
              <span className="mt-2 text-sm text-[var(--muted-foreground)]">
                {file === null ? "Originals are verified before ingest." : formatBytes(file.size)}
              </span>
              <input
                className="sr-only"
                type="file"
                accept="image/*,video/*"
                onChange={(event) => {
                  setFile(event.target.files?.[0] ?? null);
                  setProgress(null);
                }}
              />
            </label>
            {progress !== null ? (
              <div className="rounded-2xl border border-[var(--border)] bg-[var(--surface)] p-4">
                <div className="flex items-center justify-between text-sm">
                  <span className="font-medium capitalize">{progress.phase}</span>
                  <span className="text-[var(--muted-foreground)]">{percent}%</span>
                </div>
                <div className="mt-3 h-2 overflow-hidden rounded-full bg-[var(--surface-subtle)]">
                  <div
                    className="h-full rounded-full bg-[var(--primary)] transition-[width]"
                    style={{ width: `${String(percent)}%` }}
                  />
                </div>
              </div>
            ) : null}
            <Button disabled={file === null || mutation.isPending} type="submit">
              {mutation.isPending ? <Loader2 className="animate-spin" aria-hidden="true" /> : null}
              Upload asset
            </Button>
            <FormError error={mutation.error} />
          </form>
        </CardContent>
      </Card>
      <SideNote
        title="Operational note"
        items={[
          "Uploads use owner session CSRF protection.",
          "Content identity is the original BLAKE3 hash.",
          "Refresh Photos after background derivative generation."
        ]}
      />
    </PanelGrid>
  );
}

function SearchPanel(props: { onOpen: (assetId: string) => void }) {
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<"filename" | "semantic">("filename");
  const [submitted, setSubmitted] = useState("");
  const resultsQuery = useQuery({
    enabled: submitted.length > 0,
    queryFn: () => searchAssets({ query: submitted, mode, limit: 60 }),
    queryKey: ["search", submitted, mode]
  });
  const results = resultsQuery.data?.items ?? [];

  return (
    <section className="flex h-full min-h-0 flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Search the vault</CardTitle>
          <CardDescription>Use filename search by default, or semantic search when model packs are active.</CardDescription>
        </CardHeader>
        <CardContent>
          <form
            className="grid gap-3 sm:grid-cols-[1fr_auto_auto]"
            onSubmit={(event) => {
              event.preventDefault();
              setSubmitted(query.trim());
            }}
          >
            <Input
              aria-label="Search query"
              placeholder="Trip, receipt, portrait, filename"
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
              }}
            />
            <select
              className="h-10 rounded-xl border border-[var(--input)] bg-[var(--surface)] px-3 text-sm"
              aria-label="Search mode"
              value={mode}
              onChange={(event) => {
                setMode(event.target.value as "filename" | "semantic");
              }}
            >
              <option value="filename">Filename</option>
              <option value="semantic">Semantic</option>
            </select>
            <Button type="submit">
              <Search aria-hidden="true" />
              Search
            </Button>
          </form>
        </CardContent>
      </Card>
      {resultsQuery.isFetching ? <p className="text-sm text-[var(--muted-foreground)]">Searching</p> : null}
      {resultsQuery.error ? <FormError error={resultsQuery.error} /> : null}
      {submitted.length > 0 && results.length === 0 && !resultsQuery.isFetching ? (
        <EmptyState icon={<Search />} title="No matches" description="Try filename mode first, then semantic search if models are active." />
      ) : null}
      {results.length > 0 ? (
        <div className="min-h-0 flex-1 overflow-auto rounded-2xl border border-[var(--border)] bg-[var(--surface)] p-3">
          <SimpleAssetGrid assets={results} onOpen={props.onOpen} />
        </div>
      ) : null}
    </section>
  );
}

function PeoplePanel() {
  const peopleQuery = useQuery({ queryFn: listPeople, queryKey: ["people"] });
  const facesQuery = useQuery({ queryFn: () => listUnassignedFaces(24), queryKey: ["faces", "unassigned"] });

  return (
    <PanelGrid>
      <Card>
        <CardHeader>
          <CardTitle>People</CardTitle>
          <CardDescription>Review known people and face clusters.</CardDescription>
        </CardHeader>
        <CardContent>
          {peopleQuery.isPending ? <p className="text-sm text-[var(--muted-foreground)]">Loading people</p> : null}
          {peopleQuery.error ? <FormError error={peopleQuery.error} /> : null}
          <div className="grid gap-3">
            {peopleQuery.data?.map((person) => (
              <PersonRow key={person.personId} person={person} />
            ))}
            {peopleQuery.data?.length === 0 ? (
              <EmptyState icon={<Users />} title="No people yet" description="Face grouping will appear here after indexing." />
            ) : null}
          </div>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Unassigned faces</CardTitle>
          <CardDescription>Faces waiting for review.</CardDescription>
        </CardHeader>
        <CardContent>
          {facesQuery.isPending ? <p className="text-sm text-[var(--muted-foreground)]">Loading faces</p> : null}
          {facesQuery.error ? <FormError error={facesQuery.error} /> : null}
          <div className="grid grid-cols-3 gap-2 sm:grid-cols-4">
            {facesQuery.data?.map((face) => (
              <FaceChip key={face.faceId} face={face} />
            ))}
          </div>
        </CardContent>
      </Card>
    </PanelGrid>
  );
}

function ModelsPanel() {
  const [manifest, setManifest] = useState("");
  const [parseError, setParseError] = useState<string | null>(null);
  const queryClient = useQueryClient();
  const packsQuery = useQuery({ queryFn: listModelPacks, queryKey: ["model-packs"] });
  const invalidate = async () => {
    await queryClient.invalidateQueries({ queryKey: ["model-packs"] });
  };
  const installMutation = useMutation({
    mutationFn: installModelPack,
    onSuccess: invalidate
  });
  const selfTestMutation = useMutation({
    mutationFn: runModelPackSelfTest,
    onSuccess: invalidate
  });
  const activateMutation = useMutation({
    mutationFn: activateModelPack,
    onSuccess: invalidate
  });
  const reindexMutation = useMutation({ mutationFn: startModelReindex });

  return (
    <PanelGrid>
      <Card className="lg:col-span-2">
        <CardHeader>
          <CardTitle>Model packs</CardTitle>
          <CardDescription>Install, self-test, activate, and reindex AI model packs.</CardDescription>
        </CardHeader>
        <CardContent>
          {packsQuery.isPending ? <p className="text-sm text-[var(--muted-foreground)]">Loading model packs</p> : null}
          {packsQuery.error ? <FormError error={packsQuery.error} /> : null}
          <div className="grid gap-3">
            {packsQuery.data?.map((pack) => (
              <ModelPackCard
                key={pack.modelPackId}
                pack={pack}
                busy={
                  selfTestMutation.isPending ||
                  activateMutation.isPending ||
                  reindexMutation.isPending
                }
                onActivate={() => {
                  activateMutation.mutate(pack.modelPackId);
                }}
                onReindex={() => {
                  reindexMutation.mutate(pack.modelPackId);
                }}
                onSelfTest={() => {
                  selfTestMutation.mutate(pack.modelPackId);
                }}
              />
            ))}
            {packsQuery.data?.length === 0 ? (
              <EmptyState icon={<Brain />} title="No model packs installed" description="Paste a model manifest to enable semantic search and face workflows." />
            ) : null}
          </div>
          <FormError error={selfTestMutation.error ?? activateMutation.error ?? reindexMutation.error} />
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Install manifest</CardTitle>
          <CardDescription>Paste the JSON manifest returned by your model pack build.</CardDescription>
        </CardHeader>
        <CardContent>
          <form
            className="grid gap-3"
            onSubmit={(event) => {
              event.preventDefault();
              setParseError(null);
              try {
                installMutation.mutate(JSON.parse(manifest) as unknown);
              } catch {
                setParseError("Manifest must be valid JSON.");
              }
            }}
          >
            <Textarea
              aria-label="Model manifest JSON"
              placeholder='{"kind":"clip","runtime":"..."}'
              value={manifest}
              onChange={(event) => {
                setManifest(event.target.value);
              }}
            />
            <Button disabled={installMutation.isPending || manifest.trim().length === 0} type="submit">
              {installMutation.isPending ? <Loader2 className="animate-spin" aria-hidden="true" /> : null}
              Install pack
            </Button>
            {parseError !== null ? <p className="text-sm text-[var(--destructive-text)]">{parseError}</p> : null}
            <FormError error={installMutation.error} />
          </form>
        </CardContent>
      </Card>
    </PanelGrid>
  );
}

function ExportsPanel() {
  const manifestQuery = useQuery({ queryFn: getExportManifest, queryKey: ["exports", "manifest"] });
  const totalBytes = manifestQuery.data?.items.reduce((sum, item) => sum + item.sizeBytes, 0) ?? 0;

  return (
    <PanelGrid>
      <Card className="lg:col-span-2">
        <CardHeader>
          <CardTitle>Original exports</CardTitle>
          <CardDescription>Download a manifest or full archive of original media.</CardDescription>
        </CardHeader>
        <CardContent>
          {manifestQuery.isPending ? <p className="text-sm text-[var(--muted-foreground)]">Loading export manifest</p> : null}
          {manifestQuery.error ? <FormError error={manifestQuery.error} /> : null}
          {manifestQuery.data ? (
            <div className="grid gap-4">
              <div className="grid gap-3 sm:grid-cols-3">
                <MetricCard label="Items" value={String(manifestQuery.data.items.length)} icon={<Archive />} />
                <MetricCard label="Size" value={formatBytes(totalBytes)} icon={<Download />} />
                <MetricCard label="Manifest" value={manifestQuery.data.manifestVersion} icon={<FileDown />} />
              </div>
              <div className="flex flex-wrap gap-2">
                <Button asChild>
                  <a href="/exports/originals/archive.tar">
                    <Download aria-hidden="true" />
                    Download archive
                  </a>
                </Button>
                <Button variant="outline" asChild>
                  <a href="/exports/originals/manifest">
                    <FileDown aria-hidden="true" />
                    Download manifest
                  </a>
                </Button>
              </div>
              <div className="max-h-72 overflow-auto rounded-2xl border border-[var(--border)]">
                <table className="w-full text-left text-sm">
                  <thead className="sticky top-0 bg-[var(--surface-muted)] text-xs uppercase tracking-[0.12em] text-[var(--muted-foreground)]">
                    <tr>
                      <th className="px-4 py-3">File</th>
                      <th className="px-4 py-3">Type</th>
                      <th className="px-4 py-3">Size</th>
                    </tr>
                  </thead>
                  <tbody>
                    {manifestQuery.data.items.slice(0, 80).map((item) => (
                      <tr className="border-t border-[var(--border)]" key={item.assetId}>
                        <td className="px-4 py-3">{item.originalFilename ?? item.assetId}</td>
                        <td className="px-4 py-3 text-[var(--muted-foreground)]">{item.mediaType}</td>
                        <td className="px-4 py-3 text-[var(--muted-foreground)]">{formatBytes(item.sizeBytes)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          ) : null}
        </CardContent>
      </Card>
      <SideNote
        title="Export behavior"
        items={[
          "Manifest records asset IDs, hashes, media types, and storage keys.",
          "Archive endpoints stream originals from backend storage.",
          "Keep exported archives encrypted at rest."
        ]}
      />
    </PanelGrid>
  );
}

function TrashPanel() {
  const queryClient = useQueryClient();
  const trashQuery = useInfiniteQuery({
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) => listTrashedAssets(pageParam),
    queryKey: ["trash", "assets"],
    getNextPageParam: (lastPage) => lastPage.nextCursor ?? undefined
  });
  const assets = useMemo(
    () => trashQuery.data?.pages.flatMap((page) => page.items) ?? [],
    [trashQuery.data]
  );
  const restoreMutation = useMutation({
    mutationFn: restoreAsset,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["trash", "assets"] });
      await queryClient.invalidateQueries({ queryKey: ["assets"] });
    }
  });
  const purgeMutation = useMutation({
    mutationFn: purgeAsset,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["trash", "assets"] });
    }
  });

  return (
    <section className="grid gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Trash</CardTitle>
          <CardDescription>Restore assets or permanently purge items you no longer need.</CardDescription>
        </CardHeader>
        <CardContent>
          {trashQuery.isPending ? <p className="text-sm text-[var(--muted-foreground)]">Loading trash</p> : null}
          {trashQuery.error ? <FormError error={trashQuery.error} /> : null}
          {assets.length === 0 && !trashQuery.isPending ? (
            <EmptyState icon={<Trash2 />} title="Trash is empty" description="Deleted assets will appear here before purge." />
          ) : null}
          <div className="grid gap-3">
            {assets.map((asset) => (
              <AssetActionRow
                asset={asset}
                key={asset.assetId}
                primaryLabel="Restore"
                destructiveLabel="Purge"
                busy={restoreMutation.isPending || purgeMutation.isPending}
                onPrimary={() => {
                  restoreMutation.mutate(asset.assetId);
                }}
                onDestructive={() => {
                  purgeMutation.mutate(asset.assetId);
                }}
              />
            ))}
          </div>
          {trashQuery.hasNextPage ? (
            <Button
              className="mt-4"
              variant="outline"
              disabled={trashQuery.isFetchingNextPage}
              type="button"
              onClick={() => {
                void trashQuery.fetchNextPage();
              }}
            >
              Load more
            </Button>
          ) : null}
          <FormError error={restoreMutation.error ?? purgeMutation.error} />
        </CardContent>
      </Card>
    </section>
  );
}

function SessionsPanel(props: { active: boolean }) {
  const sessionsQuery = useQuery({
    enabled: props.active,
    queryFn: listSessions,
    queryKey: ["sessions"]
  });

  return (
    <section className="grid gap-4" aria-labelledby="sessions-heading">
      <Card>
        <CardHeader>
          <CardTitle id="sessions-heading">Active sessions</CardTitle>
          <CardDescription>Browsers currently signed in to Mirror.</CardDescription>
        </CardHeader>
        <CardContent>
          {sessionsQuery.isPending ? <p className="text-sm text-[var(--muted-foreground)]">Loading sessions</p> : null}
          {sessionsQuery.error ? <FormError error={sessionsQuery.error} /> : null}
          {sessionsQuery.data?.length === 0 ? (
            <p className="text-sm text-[var(--muted-foreground)]">No active sessions</p>
          ) : null}
          {sessionsQuery.data ? <SessionList sessions={sessionsQuery.data} /> : null}
        </CardContent>
      </Card>
    </section>
  );
}

function SecurityPanel() {
  const [password, setPassword] = useState("");
  const [totpCode, setTotpCode] = useState("");
  const [recoveryCode, setRecoveryCode] = useState("");
  const [tokenName, setTokenName] = useState("Import worker");
  const [setupResult, setSetupResult] = useState<{ secretBase32: string; provisioningUri: string } | null>(null);
  const [newCodes, setNewCodes] = useState<string[] | null>(null);
  const [deviceToken, setDeviceToken] = useState<string | null>(null);
  const queryClient = useQueryClient();
  const mfaQuery = useQuery({ queryFn: getMfaStatus, queryKey: ["mfa"] });
  const invalidateMfa = async () => {
    await queryClient.invalidateQueries({ queryKey: ["mfa"] });
  };
  const setupMutation = useMutation({
    mutationFn: setupTotp,
    onSuccess: (result) => {
      setSetupResult(result);
    }
  });
  const enableMutation = useMutation({
    mutationFn: enableTotp,
    onSuccess: async (codes) => {
      setNewCodes(codes);
      await invalidateMfa();
    }
  });
  const disableMutation = useMutation({
    mutationFn: disableTotp,
    onSuccess: invalidateMfa
  });
  const rotateMutation = useMutation({
    mutationFn: rotateRecoveryCodes,
    onSuccess: async (codes) => {
      setNewCodes(codes);
      await invalidateMfa();
    }
  });
  const tokenMutation = useMutation({
    mutationFn: createDeviceToken,
    onSuccess: (result) => {
      setDeviceToken(result.token);
    }
  });

  const secondFactor = { password, totpCode, recoveryCode };

  return (
    <PanelGrid>
      <Card>
        <CardHeader>
          <CardTitle>MFA</CardTitle>
          <CardDescription>Manage TOTP and recovery codes for the owner account.</CardDescription>
        </CardHeader>
        <CardContent>
          {mfaQuery.isPending ? <p className="text-sm text-[var(--muted-foreground)]">Loading MFA status</p> : null}
          {mfaQuery.error ? <FormError error={mfaQuery.error} /> : null}
          {mfaQuery.data ? (
            <div className="mb-4 grid gap-3 sm:grid-cols-3">
              <MetricCard label="TOTP" value={mfaQuery.data.totpEnabled ? "Enabled" : "Off"} icon={<LockKeyhole />} />
              <MetricCard label="Pending setup" value={mfaQuery.data.totpSetupPending ? "Yes" : "No"} icon={<ShieldCheck />} />
              <MetricCard label="Recovery codes" value={String(mfaQuery.data.recoveryCodesRemaining)} icon={<KeyRound />} />
            </div>
          ) : null}
          <div className="grid gap-3">
            <Field label="Password">
              <Input
                autoComplete="current-password"
                type="password"
                value={password}
                onChange={(event) => {
                  setPassword(event.target.value);
                }}
              />
            </Field>
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="TOTP code">
                <Input
                  autoComplete="one-time-code"
                  value={totpCode}
                  onChange={(event) => {
                    setTotpCode(event.target.value);
                  }}
                />
              </Field>
              <Field label="Recovery code">
                <Input
                  autoComplete="one-time-code"
                  value={recoveryCode}
                  onChange={(event) => {
                    setRecoveryCode(event.target.value);
                  }}
                />
              </Field>
            </div>
            <div className="flex flex-wrap gap-2">
              <Button
                variant="outline"
                disabled={password.length === 0 || setupMutation.isPending}
                type="button"
                onClick={() => {
                  setupMutation.mutate(password);
                }}
              >
                Setup TOTP
              </Button>
              <Button
                disabled={password.length === 0 || totpCode.length === 0 || enableMutation.isPending}
                type="button"
                onClick={() => {
                  enableMutation.mutate({ password, totpCode });
                }}
              >
                Enable TOTP
              </Button>
              <Button
                variant="outline"
                disabled={password.length === 0 || disableMutation.isPending}
                type="button"
                onClick={() => {
                  disableMutation.mutate(secondFactor);
                }}
              >
                Disable TOTP
              </Button>
              <Button
                variant="outline"
                disabled={password.length === 0 || rotateMutation.isPending}
                type="button"
                onClick={() => {
                  rotateMutation.mutate(secondFactor);
                }}
              >
                Rotate recovery codes
              </Button>
            </div>
          </div>
          {setupResult !== null ? (
            <pre className="mt-4 overflow-auto rounded-2xl bg-[var(--surface-muted)] p-4 text-xs">
              {setupResult.secretBase32}
              {"\n"}
              {setupResult.provisioningUri}
            </pre>
          ) : null}
          {newCodes !== null ? (
            <pre className="mt-4 overflow-auto rounded-2xl bg-[var(--surface-muted)] p-4 text-xs">
              {newCodes.join("\n")}
            </pre>
          ) : null}
          <FormError error={setupMutation.error ?? enableMutation.error ?? disableMutation.error ?? rotateMutation.error} />
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Device tokens</CardTitle>
          <CardDescription>Create a token for importers or trusted automation.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3">
            <Field label="Token name">
              <Input
                value={tokenName}
                onChange={(event) => {
                  setTokenName(event.target.value);
                }}
              />
            </Field>
            <Button
              disabled={password.length === 0 || tokenName.trim().length === 0 || tokenMutation.isPending}
              type="button"
              onClick={() => {
                tokenMutation.mutate({
                  name: tokenName,
                  password,
                  totpCode,
                  recoveryCode
                });
              }}
            >
              Create device token
            </Button>
          </div>
          {deviceToken !== null ? (
            <pre className="mt-4 overflow-auto rounded-2xl bg-[var(--surface-muted)] p-4 text-xs">
              {deviceToken}
            </pre>
          ) : null}
          <FormError error={tokenMutation.error} />
        </CardContent>
      </Card>
    </PanelGrid>
  );
}

function SystemPanel() {
  const readyQuery = useQuery({ queryFn: getReady, queryKey: ["ready"] });

  return (
    <PanelGrid>
      <Card className="lg:col-span-2">
        <CardHeader>
          <CardTitle>System readiness</CardTitle>
          <CardDescription>Backend readiness for configuration and database access.</CardDescription>
        </CardHeader>
        <CardContent>
          {readyQuery.isPending ? <p className="text-sm text-[var(--muted-foreground)]">Checking readiness</p> : null}
          {readyQuery.error ? <FormError error={readyQuery.error} /> : null}
          {readyQuery.data ? (
            <div className="grid gap-3 sm:grid-cols-3">
              <MetricCard label="Status" value={readyQuery.data.status} icon={<CheckCircle2 />} />
              <MetricCard label="Config" value={readyQuery.data.config} icon={<Server />} />
              <MetricCard label="Database" value={readyQuery.data.database} icon={<Archive />} />
            </div>
          ) : null}
        </CardContent>
      </Card>
      <SideNote
        title="Admin scope"
        items={[
          "Photos are the canonical default view.",
          "Sessions and security are owner-only operations.",
          "Models power semantic search and people review."
        ]}
      />
    </PanelGrid>
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
  const gap = 12;
  const columns = Math.max(1, Math.floor((containerWidth + gap) / (156 + gap)));
  const tileWidth = Math.max(112, (containerWidth - gap * (columns - 1)) / columns);
  const rowHeight = tileWidth + 54;
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
    fallbackRows.length === 0 ? 0 : fallbackRows[fallbackRows.length - 1]?.end ?? 0;

  return (
    <section className="asset-timeline min-h-0 flex-1 rounded-2xl border border-[var(--border)] bg-[var(--surface)] p-3">
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
          <Button
            className="load-more mt-3 w-full"
            variant="outline"
            disabled={props.isFetchingNextPage}
            type="button"
            onClick={props.onLoadMore}
          >
            {props.isFetchingNextPage ? "Loading..." : "Load more"}
          </Button>
        ) : null}
      </div>
    </section>
  );
}

function SimpleAssetGrid(props: {
  assets: AssetTimelineItem[];
  onOpen: (assetId: string) => void;
}) {
  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
      {props.assets.map((asset) => (
        <AssetTile asset={asset} key={asset.assetId} onOpen={props.onOpen} />
      ))}
    </div>
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
      className="group min-w-0 rounded-2xl p-1 text-left text-[var(--foreground)] transition-colors hover:bg-[var(--surface-muted)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--ring)]"
      type="button"
      onClick={() => {
        props.onOpen(props.asset.assetId);
      }}
    >
      <div className="relative aspect-square w-full overflow-hidden rounded-xl bg-[var(--surface-subtle)]">
        {props.asset.thumbnail === null ? (
          <div className="grid size-full place-items-center px-3 text-center text-xs text-[var(--muted-foreground)]">
            {props.asset.mediaType}
          </div>
        ) : (
          <img
            alt={label}
            className="size-full object-cover transition-transform duration-300 group-hover:scale-[1.025]"
            height={props.asset.thumbnail.height}
            loading="lazy"
            src={derivativeUrl(props.asset.assetId, "thumbnail")}
            width={props.asset.thumbnail.width}
          />
        )}
        {props.asset.mediaType.startsWith("video/") ? (
          <span className="absolute right-2 top-2 flex size-7 items-center justify-center rounded-full bg-black/55 text-white" title="Video">
            <Play aria-hidden="true" fill="currentColor" size={14} />
          </span>
        ) : null}
        {props.asset.favoriteAt !== null ? (
          <span className="absolute left-2 top-2 flex size-7 items-center justify-center rounded-full bg-white/85 text-[var(--primary-text)]" title="Favorite">
            <Heart aria-hidden="true" fill="currentColor" size={14} />
          </span>
        ) : null}
      </div>
      <span className="grid gap-1 px-1 pt-2 text-xs leading-5">
        <span className="truncate font-medium">{label}</span>
        <time className="text-[var(--muted-foreground)]" dateTime={props.asset.createdAt}>
          {formatDate(props.asset.createdAt, false)}
        </time>
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
  const queryClient = useQueryClient();
  const favoriteMutation = useMutation({
    mutationFn: props.asset.favoriteAt === null ? favoriteAsset : unfavoriteAsset,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["assets"] });
    }
  });
  const trashMutation = useMutation({
    mutationFn: trashAsset,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["assets"] });
      props.onClose();
    }
  });

  useEffect(() => {
    dialogRef.current?.focus();
  }, [props.asset.assetId]);

  return (
    <div
      aria-label={label}
      aria-modal="true"
      className="fixed inset-0 z-50 grid bg-black/88 text-white"
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
        className="absolute right-4 top-4 z-10 flex size-11 items-center justify-center rounded-full bg-white/10 text-white backdrop-blur transition-colors hover:bg-white/18"
        type="button"
        onClick={props.onClose}
      >
        <X aria-hidden="true" />
      </button>
      <button
        aria-label="Previous asset"
        className="absolute left-4 top-1/2 z-10 flex size-12 -translate-y-1/2 items-center justify-center rounded-full bg-white/10 text-white backdrop-blur transition-colors hover:bg-white/18 disabled:opacity-30"
        disabled={!props.canGoPrevious}
        type="button"
        onClick={props.onPrevious}
      >
        <ChevronLeft aria-hidden="true" />
      </button>
      <div className="grid min-h-0 place-items-center p-6 pb-28 pt-20">
        {props.asset.preview === null ? (
          <div className="grid min-h-64 min-w-64 place-items-center rounded-2xl bg-white/10 p-8 text-white/75">
            {props.asset.mediaType}
          </div>
        ) : (
          <img
            alt={`${label} preview`}
            className="max-h-full max-w-full rounded-2xl object-contain shadow-2xl"
            height={props.asset.preview.height}
            src={derivativeUrl(props.asset.assetId, "preview")}
            width={props.asset.preview.width}
          />
        )}
      </div>
      <button
        aria-label="Next asset"
        className="absolute right-4 top-1/2 z-10 flex size-12 -translate-y-1/2 items-center justify-center rounded-full bg-white/10 text-white backdrop-blur transition-colors hover:bg-white/18 disabled:opacity-30"
        disabled={!props.canGoNext}
        type="button"
        onClick={props.onNext}
      >
        <ChevronRight aria-hidden="true" />
      </button>
      <footer className="absolute inset-x-0 bottom-0 flex flex-wrap items-center justify-between gap-3 border-t border-white/10 bg-black/35 p-4 backdrop-blur">
        <div>
          <strong className="block">{label}</strong>
          <span className="text-sm text-white/70">
            {formatDate(props.asset.createdAt)} / {formatBytes(props.asset.sizeBytes)}
          </span>
        </div>
        <div className="flex gap-2">
          <Button
            variant="outline"
            className="border-white/20 bg-white/10 text-white hover:bg-white/18"
            disabled={favoriteMutation.isPending}
            type="button"
            onClick={() => {
              favoriteMutation.mutate(props.asset.assetId);
            }}
          >
            <Heart aria-hidden="true" />
            {props.asset.favoriteAt === null ? "Favorite" : "Unfavorite"}
          </Button>
          <Button
            variant="destructive"
            disabled={trashMutation.isPending}
            type="button"
            onClick={() => {
              trashMutation.mutate(props.asset.assetId);
            }}
          >
            <Trash2 aria-hidden="true" />
            Trash
          </Button>
        </div>
      </footer>
    </div>
  );
}

function SessionList(props: { sessions: Session[] }) {
  return (
    <ul className="grid gap-3">
      {props.sessions.map((session) => (
        <li className="rounded-2xl border border-[var(--border)] bg-[var(--surface-muted)] p-4" key={session.sessionId}>
          <div className="flex items-start justify-between gap-3">
            <div>
              <h3 className="font-semibold">{session.deviceName ?? "Web browser"}</h3>
              <p className="mt-1 text-sm text-[var(--muted-foreground)]">
                {session.userAgent ?? "Browser details unavailable"}
              </p>
            </div>
            {session.isCurrent ? <Badge>Current</Badge> : null}
          </div>
          <dl className="mt-4 grid gap-3 text-sm sm:grid-cols-3">
            <MetaItem label="Last active" value={formatDate(session.lastSeenAt ?? session.createdAt)} dateTime={session.lastSeenAt ?? session.createdAt} />
            <MetaItem label="Signed in" value={formatDate(session.createdAt)} dateTime={session.createdAt} />
            <MetaItem label="Expires" value={formatDate(session.expiresAt)} dateTime={session.expiresAt} />
          </dl>
        </li>
      ))}
    </ul>
  );
}

function MetaItem(props: { label: string; value: string; dateTime: string }) {
  return (
    <div>
      <dt className="text-xs uppercase tracking-[0.14em] text-[var(--muted-foreground)]">{props.label}</dt>
      <dd className="mt-1">
        <time dateTime={props.dateTime}>{props.value}</time>
      </dd>
    </div>
  );
}

function PersonRow(props: { person: { personId: string; displayName: string | null; reviewStatus: string; faceCount: number } }) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-2xl border border-[var(--border)] bg-[var(--surface-muted)] p-4">
      <div>
        <h3 className="font-semibold">{props.person.displayName ?? "Unnamed person"}</h3>
        <p className="text-sm text-[var(--muted-foreground)]">{props.person.faceCount} faces</p>
      </div>
      <Badge variant="outline">{props.person.reviewStatus}</Badge>
    </div>
  );
}

function FaceChip(props: { face: FaceAlbumItem }) {
  return (
    <div className="overflow-hidden rounded-2xl border border-[var(--border)] bg-[var(--surface-muted)]">
      {props.face.chipAvailable ? (
        <img
          alt="Face chip"
          className="aspect-square w-full object-cover"
          src={faceChipUrl(props.face.faceId)}
        />
      ) : (
        <div className="grid aspect-square place-items-center text-xs text-[var(--muted-foreground)]">
          No chip
        </div>
      )}
      <div className="p-2 text-xs text-[var(--muted-foreground)]">{props.face.reviewState}</div>
    </div>
  );
}

function ModelPackCard(props: {
  pack: ModelPackSummary;
  busy: boolean;
  onActivate: () => void;
  onReindex: () => void;
  onSelfTest: () => void;
}) {
  return (
    <div className="rounded-2xl border border-[var(--border)] bg-[var(--surface-muted)] p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="font-semibold">{props.pack.modelKey}</h3>
          <p className="mt-1 text-sm text-[var(--muted-foreground)]">
            {props.pack.kind} / {props.pack.runtime} / {props.pack.modelRevision}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Badge variant={props.pack.status === "active" ? "default" : "secondary"}>
            {props.pack.status}
          </Badge>
          <Badge variant="outline">{props.pack.selfTestStatus}</Badge>
        </div>
      </div>
      <dl className="mt-4 grid gap-3 text-sm sm:grid-cols-3">
        <div>
          <dt className="text-[var(--muted-foreground)]">Dimensions</dt>
          <dd>{props.pack.embeddingDimension}</dd>
        </div>
        <div>
          <dt className="text-[var(--muted-foreground)]">Distance</dt>
          <dd>{props.pack.distanceMetric}</dd>
        </div>
        <div>
          <dt className="text-[var(--muted-foreground)]">Updated</dt>
          <dd>{formatDate(props.pack.updatedAt, false)}</dd>
        </div>
      </dl>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button variant="outline" disabled={props.busy} type="button" onClick={props.onSelfTest}>
          Self-test
        </Button>
        <Button variant="outline" disabled={props.busy} type="button" onClick={props.onReindex}>
          Reindex
        </Button>
        <Button disabled={props.busy || props.pack.status === "active"} type="button" onClick={props.onActivate}>
          Activate
        </Button>
      </div>
    </div>
  );
}

function AssetActionRow(props: {
  asset: AssetTimelineItem;
  primaryLabel: string;
  destructiveLabel: string;
  busy: boolean;
  onPrimary: () => void;
  onDestructive: () => void;
}) {
  const label = props.asset.originalFilename ?? props.asset.assetId;
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--border)] bg-[var(--surface-muted)] p-3">
      <div className="flex min-w-0 items-center gap-3">
        <div className="size-14 overflow-hidden rounded-xl bg-[var(--surface-subtle)]">
          {props.asset.thumbnail !== null ? (
            <img
              alt={label}
              className="size-full object-cover"
              src={trashedDerivativeUrl(props.asset.assetId, "thumbnail")}
            />
          ) : null}
        </div>
        <div className="min-w-0">
          <p className="truncate font-medium">{label}</p>
          <p className="text-sm text-[var(--muted-foreground)]">
            {formatDate(props.asset.createdAt, false)} / {formatBytes(props.asset.sizeBytes)}
          </p>
        </div>
      </div>
      <div className="flex gap-2">
        <Button variant="outline" disabled={props.busy} type="button" onClick={props.onPrimary}>
          {props.primaryLabel}
        </Button>
        <Button variant="destructive" disabled={props.busy} type="button" onClick={props.onDestructive}>
          {props.destructiveLabel}
        </Button>
      </div>
    </div>
  );
}

function MetricCard(props: { label: string; value: string; icon: ReactNode }) {
  return (
    <Card className="shadow-none">
      <CardContent className="flex items-center gap-3 p-4">
        <span className="flex size-10 shrink-0 items-center justify-center rounded-2xl bg-[var(--primary-soft)] text-[var(--primary-text)] [&_svg]:size-5">
          {props.icon}
        </span>
        <div className="min-w-0">
          <p className="truncate text-xs font-medium uppercase tracking-[0.14em] text-[var(--muted-foreground)]">
            {props.label}
          </p>
          <p className="mt-1 truncate text-xl font-semibold tracking-[-0.035em]">{props.value}</p>
        </div>
      </CardContent>
    </Card>
  );
}

function EmptyState(props: { icon: ReactNode; title: string; description: string }) {
  return (
    <div className="grid place-items-center rounded-2xl border border-dashed border-[var(--border-strong)] bg-[var(--surface)] p-10 text-center">
      <span className="mb-3 flex size-12 items-center justify-center rounded-2xl bg-[var(--surface-muted)] text-[var(--muted-foreground)] [&_svg]:size-6">
        {props.icon}
      </span>
      <h3 className="font-semibold">{props.title}</h3>
      <p className="mt-2 max-w-sm text-sm leading-6 text-[var(--muted-foreground)]">{props.description}</p>
    </div>
  );
}

function SideNote(props: { title: string; items: string[] }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{props.title}</CardTitle>
      </CardHeader>
      <CardContent>
        <ul className="grid gap-3 text-sm text-[var(--muted-foreground)]">
          {props.items.map((item) => (
            <li className="flex gap-2" key={item}>
              <Sparkles className="mt-0.5 size-4 shrink-0 text-[var(--primary-text)]" aria-hidden="true" />
              <span>{item}</span>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}

function PanelGrid(props: { children: ReactNode }) {
  return <section className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_360px]">{props.children}</section>;
}

function Field(props: { label: string; children: ReactNode }) {
  return (
    <label className="grid gap-2 text-sm font-medium text-[var(--foreground)]">
      <span>{props.label}</span>
      {props.children}
    </label>
  );
}

function FormError(props: { error: unknown }) {
  if (props.error === null || props.error === undefined) {
    return null;
  }
  const message = props.error instanceof ApiError ? props.error.message : "Something went wrong";
  return (
    <p className="mt-3 rounded-xl border border-[var(--destructive-border)] bg-[var(--destructive-soft)] px-3 py-2 text-sm text-[var(--destructive-text)]">
      {message}
    </p>
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

function sumBytes(assets: AssetTimelineItem[]): number {
  return assets.reduce((sum, asset) => sum + asset.sizeBytes, 0);
}

const viewTitles: Record<AdminView, string> = {
  photos: "Photos",
  upload: "Upload",
  search: "Search",
  people: "People",
  models: "Models",
  exports: "Exports",
  trash: "Trash",
  sessions: "Sessions",
  security: "Security",
  system: "System"
};
