import { Outlet, ScrollRestoration } from 'react-router'

/** Top route element: restores scroll position on navigation. */
export function Root() {
  return (
    <>
      <Outlet />
      <ScrollRestoration />
    </>
  )
}
