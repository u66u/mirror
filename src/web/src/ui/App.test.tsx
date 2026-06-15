import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";

afterEach(() => {
  vi.unstubAllGlobals();
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
                {
                  asset_id: "019b0000-0000-7000-8000-000000000001",
                  created_at: "2026-06-07T10:00:00Z",
                  favorite_at: null,
                  original_blake3: "a".repeat(64),
                  media_type: "image/jpeg",
                  size_bytes: 14,
                  original_filename: "route.jpg",
                  thumbnail: { format: "webp", width: 512, height: 384 },
                  preview: { format: "webp", width: 1600, height: 1200 }
                }
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
