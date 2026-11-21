# Browser write sessions retain in-flight writes

Browser writes use a browser-owned copy to bound memory use and amortize promise
overhead. Accepted bytes belong to the session even if the caller's future is
canceled. When a write fails, cleanup starts immediately while the original
write error is retained.
