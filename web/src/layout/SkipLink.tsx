/** First focusable element on every page; jumps to `<main id="main">`. */
export function SkipLink() {
  return (
    <a href="#main" className="sr-only z-50 rounded-md bg-focus px-4 py-3 font-semibold text-ink focus:not-sr-only focus:fixed focus:top-2 focus:left-2">
      Skip to main content
    </a>
  )
}
