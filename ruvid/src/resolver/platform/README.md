# Platform Resolver Boundary

Platform-specific resolvers live here, strictly separated from core HTTP/HLS/DASH transport.

Rules:
- Implement `PlatformResolver` to resolve public media URLs or metadata from web pages.
- Do not add DRM circumvention or access-control bypass logic.
- Never log cookies, authorization headers, or secret query tokens.
