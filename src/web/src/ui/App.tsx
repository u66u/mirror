import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { ApiError, derivativeUrl, listAssets, login, setupOwner } from "../api/client";

type Mode = "setup" | "login" | "timeline";

export function App() {
  const [mode, setMode] = useState<Mode>("setup");

  if (mode === "timeline") {
    return (
      <Timeline
        onLogout={() => {
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
  const query = useQuery({
    queryFn: () => listAssets(),
    queryKey: ["assets"]
  });

  return (
    <main className="timeline-shell">
      <header className="timeline-header">
        <div>
          <h1>Mirror</h1>
          <p>{query.data?.items.length ?? 0} assets</p>
        </div>
        <button className="secondary" type="button" onClick={props.onLogout}>
          Lock
        </button>
      </header>
      {query.isPending ? <p className="muted">Loading</p> : null}
      {query.error ? <FormError error={query.error} /> : null}
      {query.data?.items.length === 0 ? <p className="muted">No assets yet</p> : null}
      <section className="asset-grid" aria-label="Asset timeline">
        {query.data?.items.map((asset) => (
          <article className="asset-tile" key={asset.assetId}>
            {asset.thumbnail === null ? (
              <div className="asset-placeholder">{asset.mediaType}</div>
            ) : (
              <img
                alt={asset.originalFilename ?? "Asset thumbnail"}
                height={asset.thumbnail.height}
                loading="lazy"
                src={derivativeUrl(asset.assetId, "thumbnail")}
                width={asset.thumbnail.width}
              />
            )}
            <div className="asset-meta">
              <span>{asset.originalFilename ?? asset.assetId}</span>
              <time dateTime={asset.createdAt}>
                {new Intl.DateTimeFormat(undefined, {
                  day: "2-digit",
                  month: "short",
                  year: "numeric"
                }).format(new Date(asset.createdAt))}
              </time>
            </div>
          </article>
        ))}
      </section>
    </main>
  );
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
