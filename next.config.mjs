/** @type {import('next').NextConfig} */
const hostBase = (
  process.env.AGENT_START_HOST_URL ?? "http://127.0.0.1:3030"
).replace(/\/+$/, "");

const nextConfig = {
  reactStrictMode: true,
  async rewrites() {
    return [
      { source: "/api/:path*", destination: `${hostBase}/api/:path*` },
      { source: "/v1/:path*", destination: `${hostBase}/v1/:path*` },
      { source: "/ws/:path*", destination: `${hostBase}/ws/:path*` },
    ];
  },
  async headers() {
    return [
      {
        // HTML pages: never cache, so the latest JS chunk references are always fetched
        source: "/((?!_next/static|api).*)",
        headers: [
          {
            key: "Cache-Control",
            value: "no-store, must-revalidate",
          },
        ],
      },
    ];
  },
};

export default nextConfig;
