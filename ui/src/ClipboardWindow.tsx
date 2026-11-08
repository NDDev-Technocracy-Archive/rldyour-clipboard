import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  DesktopAction,
  DesktopEntry,
  DesktopResult,
  View,
} from "../generated/api/types.gen";
import { type Language, languageNames } from "./localization";
import "./style.css";

export type HistorySource = (
  request: DesktopAction,
  signal: AbortSignal,
) => Promise<DesktopResult>;
type Page = Extract<DesktopResult, { result: "page" }>;
type Props = {
  source: HistorySource;
  language: Language;
  changeLanguage: (language: Language) => void;
  close: () => void;
  thumbnail?: (entry: DesktopEntry) => string | undefined;
};

export function ClipboardWindow({
  source,
  language,
  changeLanguage,
  close,
  thumbnail,
}: Props) {
  const { t } = useTranslation();
  const prefix = useId();
  const [browse, setBrowse] = useState<{ view: View; query: string }>({
    view: "recent",
    query: "",
  });
  const { view, query } = browse;
  const [page, setPage] = useState<Page>({
    result: "page",
    items: [],
    more: false,
  });
  const [busy, setBusy] = useState(true);
  const [failed, setFailed] = useState(false);
  const [notice, setNotice] = useState("");
  const work = useRef<AbortController | null>(null);
  const requestEpoch = useRef(0);
  const tabs = useRef<Array<HTMLButtonElement | null>>([]);
  const views: View[] = ["recent", "history", "favorites"];

  useEffect(() => () => work.current?.abort(), []);

  useEffect(() => {
    const controller = new AbortController();
    work.current?.abort();
    work.current = controller;
    const epoch = ++requestEpoch.current;
    setBusy(true);
    setFailed(false);
    setPage({ result: "page", items: [], more: false });
    source({ action: "browse", ...browse, before: null }, controller.signal)
      .then((result) => {
        if (result.result !== "page")
          throw new Error("Unexpected history response");
        if (requestEpoch.current === epoch && !controller.signal.aborted)
          setPage(result);
      })
      .catch(() => {
        if (!controller.signal.aborted) setFailed(true);
      })
      .finally(() => {
        if (!controller.signal.aborted) setBusy(false);
      });
    return () => controller.abort();
  }, [source, browse]);

  async function act(request: DesktopAction, message: string) {
    if (busy) return;
    const controller = new AbortController();
    work.current?.abort();
    work.current = controller;
    setBusy(true);
    setFailed(false);
    try {
      await source(request, controller.signal);
      if (controller.signal.aborted) return;
      setNotice(message);
      setBrowse((value) => ({ ...value }));
    } catch {
      if (!controller.signal.aborted) setFailed(true);
    } finally {
      if (!controller.signal.aborted) setBusy(false);
    }
  }

  async function more() {
    const last = page.items.at(-1);
    if (busy || !last) return;
    const controller = new AbortController();
    work.current?.abort();
    work.current = controller;
    const epoch = requestEpoch.current;
    setBusy(true);
    try {
      const result = await source(
        { action: "browse", view, query, before: last.key },
        controller.signal,
      );
      if (
        !controller.signal.aborted &&
        epoch === requestEpoch.current &&
        result.result === "page"
      ) {
        setPage((old) => ({
          ...result,
          items: [
            ...old.items,
            ...result.items.filter(
              (entry) => !old.items.some((item) => item.key === entry.key),
            ),
          ],
        }));
      }
    } catch {
      if (!controller.signal.aborted) setFailed(true);
    } finally {
      if (!controller.signal.aborted) setBusy(false);
    }
  }

  const empty = page.items.length === 0 && !busy && !failed;
  return (
    <main
      className="clipboard-window"
      lang={language}
      dir={language === "ar" ? "rtl" : "ltr"}
      onKeyDown={(event) => {
        if (event.key === "Escape") close();
      }}
    >
      <header className="window-header">
        <span className="product-mark" aria-hidden="true" />
        <div className="window-heading">
          <span className="product-name">NDDev Clipboard</span>
          <h1>{t("title")}</h1>
        </div>
        <button
          type="button"
          className="nd-button nd-button--quiet icon-button"
          aria-label={t("close")}
          onClick={close}
        >
          <span aria-hidden="true">×</span>
        </button>
      </header>
      <div className="search-area nd-field">
        <label className="nd-visually-hidden" htmlFor={`${prefix}-search`}>
          {t("search_label")}
        </label>
        <input
          id={`${prefix}-search`}
          className="nd-field__control"
          type="search"
          placeholder={t("search_placeholder")}
          value={query}
          onChange={(event) =>
            setBrowse({ view, query: event.currentTarget.value })
          }
          autoComplete="off"
        />
      </div>
      <div className="history-tabs" role="tablist" aria-label={t("title")}>
        {views.map((value, index) => (
          <button
            key={value}
            type="button"
            role="tab"
            id={`${prefix}-${value}`}
            aria-controls={`${prefix}-entries`}
            aria-selected={view === value}
            tabIndex={view === value ? 0 : -1}
            ref={(element) => {
              tabs.current[index] = element;
            }}
            onClick={() => setBrowse({ view: value, query })}
            onKeyDown={(event) => {
              let step =
                event.key === "ArrowRight"
                  ? 1
                  : event.key === "ArrowLeft"
                    ? -1
                    : 0;
              if (language === "ar") step *= -1;
              if (step) {
                event.preventDefault();
                const next = (index + step + views.length) % views.length;
                setBrowse({ view: views[next], query });
                tabs.current[next]?.focus();
              }
            }}
          >
            {t(value)}
          </button>
        ))}
      </div>
      <section
        id={`${prefix}-entries`}
        className="entries-panel"
        role="tabpanel"
        aria-labelledby={`${prefix}-${view}`}
        aria-busy={busy}
      >
        {failed && (
          <div className="empty-state" role="alert">
            <h2>{t("error_title")}</h2>
            <button
              type="button"
              className="nd-button nd-button--secondary"
              onClick={() => setBrowse((value) => ({ ...value }))}
            >
              {t("retry")}
            </button>
          </div>
        )}
        {empty && (
          <div className="empty-state">
            <span className="empty-symbol" aria-hidden="true">
              {view === "favorites" ? "☆" : "⌘"}
            </span>
            <h2>
              {t(
                query
                  ? "no_results"
                  : view === "favorites"
                    ? "favorites_empty"
                    : "empty_title",
              )}
            </h2>
            <p>
              {t(
                query
                  ? "no_results_body"
                  : view === "favorites"
                    ? "favorites_body"
                    : "empty_body",
              )}
            </p>
          </div>
        )}
        <ul
          className="entries-list"
          aria-label={t("entries_count", { count: page.items.length })}
        >
          {page.items.map((entry) => {
            const image = thumbnail?.(entry);
            const caption =
              entry.preview ||
              t(
                ["text", "link", "color", "image", "files"].includes(entry.kind)
                  ? entry.kind
                  : "text",
              );
            return (
              <li className="entry" key={entry.key}>
                <button
                  type="button"
                  className="entry-restore"
                  disabled={busy}
                  aria-label={`${t("copy_entry")}: ${caption}`}
                  onClick={() => {
                    void act(
                      { action: "restore", key: entry.key },
                      t("ready_to_paste"),
                    );
                  }}
                >
                  <span
                    className={`entry-visual entry-visual--${entry.kind}`}
                    aria-hidden="true"
                  >
                    {image ? (
                      <img src={image} alt="" />
                    ) : (
                      <span className="entry-placeholder" />
                    )}
                  </span>
                  <span className="entry-body">
                    <span className="entry-preview" dir="auto">
                      {caption}
                    </span>
                    <span className="entry-meta">
                      <span>
                        {t(
                          ["text", "link", "color", "image", "files"].includes(
                            entry.kind,
                          )
                            ? entry.kind
                            : "text",
                        )}
                      </span>
                      <time dateTime={new Date(entry.at * 1000).toISOString()}>
                        {new Intl.DateTimeFormat(language, {
                          day: "numeric",
                          month: "short",
                          hour: "2-digit",
                          minute: "2-digit",
                        }).format(entry.at * 1000)}
                      </time>
                    </span>
                  </span>
                </button>
                <button
                  type="button"
                  className="entry-pin nd-button nd-button--quiet icon-button"
                  disabled={busy}
                  aria-pressed={entry.pinned}
                  aria-label={`${t(entry.pinned ? "unpin_entry" : "pin_entry")}: ${caption}`}
                  onClick={() => {
                    void act(
                      {
                        action: "favorite",
                        key: entry.key,
                        pinned: !entry.pinned,
                      },
                      t(entry.pinned ? "favorite_removed" : "favorite_saved"),
                    );
                  }}
                >
                  <span aria-hidden="true">{entry.pinned ? "★" : "☆"}</span>
                </button>
              </li>
            );
          })}
        </ul>
        {page.more && (
          <button
            type="button"
            className="nd-button nd-button--quiet more-button"
            disabled={busy}
            onClick={() => {
              void more();
            }}
          >
            {t("more")}
          </button>
        )}
      </section>
      <footer className="window-footer">
        <span>{t("footer")}</span>
        <label>
          <span className="nd-visually-hidden">{t("language")}</span>
          <select
            aria-label={t("language")}
            value={language}
            onChange={(event) =>
              changeLanguage(event.currentTarget.value as Language)
            }
          >
            {Object.entries(languageNames).map(([key, value]) => (
              <option key={key} value={key}>
                {value}
              </option>
            ))}
          </select>
        </label>
      </footer>
      <div className="nd-visually-hidden" role="status" aria-live="polite">
        {notice}
      </div>
    </main>
  );
}
