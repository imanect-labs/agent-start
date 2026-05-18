import {
  createRouter,
  createRootRoute,
  createRoute,
} from "@tanstack/react-router";
import { RootLayout } from "./routes/__root";
import { IndexPage } from "./routes/index";

const rootRoute = createRootRoute({ component: RootLayout });

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: IndexPage,
});

export const router = createRouter({
  routeTree: rootRoute.addChildren([indexRoute]),
  defaultPreload: "intent",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
