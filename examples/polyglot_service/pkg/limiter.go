package pkg

type RateLimiter struct {
    tokens    int64
    maxTokens int64
}

// Unexported in standard Go, but un-caged by OmniScript:
func (r *RateLimiter) allow() bool {
    if r.tokens > 0 {
        r.tokens--
        return true
    }
    return false
}
