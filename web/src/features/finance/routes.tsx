import type { RouteObject } from 'react-router'
export const publicRoutes: RouteObject[] = []
export const residentRoutes: RouteObject[] = []
export const staffRoutes: RouteObject[] = [
  { path: 'finance', lazy: async () => ({ Component: (await import('./FinancePage')).FinancePage }) },
  { path: 'finance/statements', lazy: async () => ({ Component: (await import('./StatementsPage')).StatementsPage }) },
  { path: 'finance/unmatched', lazy: async () => ({ Component: (await import('./UnmatchedPage')).UnmatchedPage }) },
  { path: 'finance/refunds', lazy: async () => ({ Component: (await import('./RefundsPage')).RefundsPage }) },
  { path: 'finance/deposits', lazy: async () => ({ Component: (await import('./DepositsPage')).DepositsPage }) },
  { path: 'finance/prices', lazy: async () => ({ Component: (await import('./PricesPage')).PricesPage }) },
]
export const adminRoutes: RouteObject[] = [{ path: 'prices', lazy: async () => ({ Component: (await import('./PricesPage')).PricesPage }) }]
