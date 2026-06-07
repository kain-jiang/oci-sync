package web

import (
	"encoding/json"
	"fmt"
	"io/fs"
	"net/http"
	"time"

	"charm.land/log/v2"
)

func corsMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func loggingMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		next.ServeHTTP(w, r)
		log.Info("HTTP request",
			"method", r.Method,
			"path", r.URL.Path,
			"duration", time.Since(start).String(),
		)
	})
}

func writeJSON(w http.ResponseWriter, status int, data any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if err := json.NewEncoder(w).Encode(data); err != nil {
		log.Error("Failed to encode JSON response", "error", err)
	}
}

func writeError(w http.ResponseWriter, status int, msg string) {
	writeJSON(w, status, map[string]string{"error": msg})
}

func serveStaticFS(mux *http.ServeMux, staticFS fs.FS) {
	fileServer := http.FileServer(http.FS(staticFS))
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		path := r.URL.Path
		if path == "/" {
			fileServer.ServeHTTP(w, r)
			return
		}
		// Try to open the file; if not found, fall back to index.html (SPA routing)
		f, err := staticFS.Open(path[1:]) // strip leading "/"
		if err != nil {
			r.URL.Path = "/"
			fileServer.ServeHTTP(w, r)
			return
		}
		f.Close()
		fileServer.ServeHTTP(w, r)
	})
}

func serveNoFrontend(mux *http.ServeMux) {
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/" {
			writeError(w, http.StatusNotFound, "Not found")
			return
		}
		writeError(w, http.StatusNotFound, "Frontend not built. Run: cd web && bun run build")
	})
}

func NewServer(port int, dev bool, staticFS fs.FS) *http.Server {
	mux := http.NewServeMux()

	mux.HandleFunc("GET /api/shortcuts", handleShortcuts)
	mux.HandleFunc("GET /api/artifacts", handleArtifacts)
	mux.HandleFunc("POST /api/push", handlePush)
	mux.HandleFunc("POST /api/pull", handlePull)
	mux.HandleFunc("POST /api/delete", handleDelete)

	if !dev {
		if staticFS != nil {
			log.Info("Serving embedded frontend")
			serveStaticFS(mux, staticFS)
		} else {
			log.Warn("No embedded frontend found")
			serveNoFrontend(mux)
		}
	} else {
		mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
			if r.URL.Path != "/" {
				writeError(w, http.StatusNotFound, "Not found")
				return
			}
			writeJSON(w, http.StatusOK, map[string]string{
				"service": "oci-sync-web",
				"mode":    "dev",
				"message": "Frontend running separately. Start with: cd web && bun run dev",
			})
		})
	}

	var handler http.Handler = mux
	handler = loggingMiddleware(handler)
	if dev {
		handler = corsMiddleware(handler)
	}

	return &http.Server{
		Addr:    fmt.Sprintf(":%d", port),
		Handler: handler,
	}
}
