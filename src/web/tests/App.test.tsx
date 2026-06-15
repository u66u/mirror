import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "../src/ui/App";

afterEach(() => {
  vi.unstubAllGlobals();
  document.cookie = "mirror_csrf=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/";
});

describe("App", () => {
  it("opens on usable owner setup form", () => {
    renderApp();

    expect(screen.getByRole("heading", { name: "Mirror" })).toBeInTheDocument();
    expect(screen.getByLabelText("Setup token")).toBeInTheDocument();
    expect(screen.getByLabelText("Display name")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toHaveAttribute("autocomplete", "new-password");
  });

  it("shows timeline after login with derivative thumbnail urls", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn((input: RequestInfo | URL) => {
        const url =
          typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
        if (url === "/auth/login") {
          return Promise.resolve(new Response(null, { status: 204 }));
        }
        if (url === "/assets?limit=60") {
          return Promise.resolve(
            Response.json({
              items: [
                assetDto(
                  "019b0000-0000-7000-8000-000000000001",
                  "route.jpg",
                  "2026-06-07T10:00:00Z"
                )
              ],
              next_cursor: null
            })
          );
        }
        return Promise.resolve(
          Response.json({ error: "internal_error", message: "unexpected request" }, { status: 500 })
        );
      })
    );
    renderApp();

    fireEvent.click(screen.getByRole("button", { name: "Login" }));
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "correct horse battery staple" }
    });
    fireEvent.click(screen.getAllByRole("button", { name: "Login" })[1]);

    await waitFor(() => {
      expect(screen.getByText("route.jpg")).toBeInTheDocument();
    });
    expect(screen.getByRole("img", { name: "route.jpg" })).toHaveAttribute(
      "src",
      "/assets/019b0000-0000-7000-8000-000000000001/derivatives/thumbnail"
    );
    fireEvent.click(screen.getByRole("button", { name: "Open route.jpg" }));
    expect(screen.getByRole("img", { name: "route.jpg preview" })).toHaveAttribute(
      "src",
      "/assets/019b0000-0000-7000-8000-000000000001/derivatives/preview"
    );
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(screen.queryByRole("dialog", { name: "route.jpg" })).not.toBeInTheDocument();
  });

  it("loads the next cursor page without replacing existing assets", async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const url = requestUrl(input);
      if (url === "/auth/login") {
        return Promise.resolve(new Response(null, { status: 204 }));
      }
      if (url === "/assets?limit=60") {
        return Promise.resolve(
          Response.json({
            items: [
              assetDto(
                "019b0000-0000-7000-8000-000000000021",
                "first.jpg",
                "2026-06-08T10:00:00Z"
              )
            ],
            next_cursor: "cursor-2"
          })
        );
      }
      if (url === "/assets?limit=60&cursor=cursor-2") {
        return Promise.resolve(
          Response.json({
            items: [
              assetDto(
                "019b0000-0000-7000-8000-000000000020",
                "second.jpg",
                "2026-06-07T10:00:00Z"
              )
            ],
            next_cursor: null
          })
        );
      }
      return unexpectedResponse();
    });
    vi.stubGlobal("fetch", fetchMock);
    renderApp();
    logIn();

    await screen.findByText("first.jpg");
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));

    expect(await screen.findByText("second.jpg")).toBeInTheDocument();
    expect(screen.getByText("first.jpg")).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith(
      "/assets?limit=60&cursor=cursor-2",
      expect.objectContaining({ credentials: "include", method: "GET" })
    );
  });

  it("sends CSRF-protected logout and returns to login only after success", async () => {
    let logoutRequest: RequestInit | undefined;
    document.cookie = "mirror_csrf=csrf-token-123; path=/";
    vi.stubGlobal(
      "fetch",
      vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
        const url = requestUrl(input);
        if (url === "/auth/login" || url === "/auth/logout") {
          if (url === "/auth/logout") {
            logoutRequest = init;
          }
          return Promise.resolve(new Response(null, { status: 204 }));
        }
        if (url === "/assets?limit=60") {
          return Promise.resolve(Response.json({ items: [], next_cursor: null }));
        }
        return unexpectedResponse();
      })
    );
    renderApp();
    logIn();

    await screen.findByText("No assets yet");
    fireEvent.click(screen.getByRole("button", { name: "Log out" }));

    await waitFor(() => {
      expect(screen.getByLabelText("Password")).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Log out" })).not.toBeInTheDocument();
    });
    expect(logoutRequest?.method).toBe("POST");
    expect(logoutRequest?.credentials).toBe("include");
    expect(new Headers(logoutRequest?.headers).get("x-csrf-token")).toBe("csrf-token-123");
  });

  it("keeps the authenticated view when logout fails", async () => {
    document.cookie = "mirror_csrf=stale-token; path=/";
    vi.stubGlobal(
      "fetch",
      vi.fn((input: RequestInfo | URL) => {
        const url = requestUrl(input);
        if (url === "/auth/login") {
          return Promise.resolve(new Response(null, { status: 204 }));
        }
        if (url === "/assets?limit=60") {
          return Promise.resolve(Response.json({ items: [], next_cursor: null }));
        }
        if (url === "/auth/logout") {
          return Promise.resolve(
            Response.json({ error: "csrf_invalid", message: "invalid csrf token" }, { status: 401 })
          );
        }
        return unexpectedResponse();
      })
    );
    renderApp();
    logIn();

    await screen.findByText("No assets yet");
    fireEvent.click(screen.getByRole("button", { name: "Log out" }));

    expect(await screen.findByText("invalid csrf token")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log out" })).toBeInTheDocument();
  });

  it("lists active sessions and identifies the current browser", async () => {
    let sessionsRequest: RequestInit | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
        const url = requestUrl(input);
        if (url === "/auth/login") {
          return Promise.resolve(new Response(null, { status: 204 }));
        }
        if (url === "/assets?limit=60") {
          return Promise.resolve(Response.json({ items: [], next_cursor: null }));
        }
        if (url === "/sessions") {
          sessionsRequest = init;
          return Promise.resolve(
            Response.json([
              {
                session_id: "019b0000-0000-7000-8000-000000000010",
                device_name: "Workstation",
                user_agent: "Firefox",
                created_at: "2026-06-10T08:00:00Z",
                last_seen_at: "2026-06-15T09:30:00Z",
                expires_at: "2026-07-10T08:00:00Z",
                is_current: true
              },
              {
                session_id: "019b0000-0000-7000-8000-000000000011",
                device_name: null,
                user_agent: null,
                created_at: "2026-06-08T08:00:00Z",
                last_seen_at: null,
                expires_at: "2026-07-08T08:00:00Z",
                is_current: false
              }
            ])
          );
        }
        return unexpectedResponse();
      })
    );
    renderApp();
    logIn();

    await screen.findByText("No assets yet");
    fireEvent.click(screen.getByRole("tab", { name: "Sessions" }));

    expect(await screen.findByText("Workstation")).toBeInTheDocument();
    expect(screen.getByText("Web browser")).toBeInTheDocument();
    expect(screen.getByText("Current")).toBeInTheDocument();
    expect(screen.getByText("Browser details unavailable")).toBeInTheDocument();
    expect(sessionsRequest?.method).toBe("GET");
    expect(sessionsRequest?.credentials).toBe("include");
  });

  it("shows session inventory errors without hiding authenticated controls", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn((input: RequestInfo | URL) => {
        const url = requestUrl(input);
        if (url === "/auth/login") {
          return Promise.resolve(new Response(null, { status: 204 }));
        }
        if (url === "/assets?limit=60") {
          return Promise.resolve(Response.json({ items: [], next_cursor: null }));
        }
        if (url === "/sessions") {
          return Promise.resolve(
            Response.json(
              { error: "database_unavailable", message: "database is unavailable" },
              { status: 503 }
            )
          );
        }
        return unexpectedResponse();
      })
    );
    renderApp();
    logIn();

    await screen.findByText("No assets yet");
    fireEvent.click(screen.getByRole("tab", { name: "Sessions" }));

    expect(await screen.findByText("database is unavailable")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log out" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Active sessions" })).toBeInTheDocument();
  });
});

function renderApp() {
  render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: {
            queries: { retry: false },
            mutations: { retry: false }
          }
        })
      }
    >
      <App />
    </QueryClientProvider>
  );
}

function logIn() {
  fireEvent.click(screen.getByRole("button", { name: "Login" }));
  fireEvent.change(screen.getByLabelText("Password"), {
    target: { value: "correct horse battery staple" }
  });
  fireEvent.click(screen.getAllByRole("button", { name: "Login" })[1]);
}

function requestUrl(input: RequestInfo | URL): string {
  return typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
}

function unexpectedResponse(): Promise<Response> {
  return Promise.resolve(
    Response.json({ error: "internal_error", message: "unexpected request" }, { status: 500 })
  );
}

function assetDto(assetId: string, filename: string, createdAt: string) {
  return {
    asset_id: assetId,
    created_at: createdAt,
    favorite_at: null,
    original_blake3: "a".repeat(64),
    media_type: "image/jpeg",
    size_bytes: 14,
    original_filename: filename,
    thumbnail: { format: "webp", width: 512, height: 384 },
    preview: { format: "webp", width: 1600, height: 1200 }
  };
}
