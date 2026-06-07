package web

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"

	"github.com/tiramission/oci-sync/internal/archive"
	"github.com/tiramission/oci-sync/internal/config"
	"github.com/tiramission/oci-sync/internal/crypto"
	"github.com/tiramission/oci-sync/internal/oci"
)

func handleShortcuts(w http.ResponseWriter, r *http.Request) {
	shortcuts := config.GetAllShortcuts()
	type shortcutJSON struct {
		Name string `json:"name"`
		Repo string `json:"repo"`
	}
	result := make([]shortcutJSON, len(shortcuts))
	for i, s := range shortcuts {
		result[i] = shortcutJSON{Name: s.Name, Repo: s.Repo}
	}
	writeJSON(w, http.StatusOK, result)
}

func handleArtifacts(w http.ResponseWriter, r *http.Request) {
	repo := r.URL.Query().Get("repo")
	if repo == "" {
		writeError(w, http.StatusBadRequest, "query parameter 'repo' is required")
		return
	}

	artifacts, err := oci.List(r.Context(), repo)
	if err != nil {
		writeError(w, http.StatusInternalServerError, fmt.Sprintf("list failed: %v", err))
		return
	}
	writeJSON(w, http.StatusOK, artifacts)
}

type pushRequest struct {
	Repo       string   `json:"repo"`
	Tag        string   `json:"tag"`
	Passphrase string   `json:"passphrase,omitempty"`
	Labels     []string `json:"labels,omitempty"`
}

func handlePush(w http.ResponseWriter, r *http.Request) {
	if err := r.ParseMultipartForm(100 << 20); err != nil {
		writeError(w, http.StatusBadRequest, fmt.Sprintf("parse multipart form: %v", err))
		return
	}

	repo := r.FormValue("repo")
	tag := r.FormValue("tag")
	passphrase := r.FormValue("passphrase")
	labelsJSON := r.FormValue("labels")

	if repo == "" || tag == "" {
		writeError(w, http.StatusBadRequest, "repo and tag are required")
		return
	}

	ref := repo + ":" + tag

	var labels []string
	if labelsJSON != "" {
		if err := json.Unmarshal([]byte(labelsJSON), &labels); err != nil {
			writeError(w, http.StatusBadRequest, "invalid labels JSON")
			return
		}
	}

	labelMap := make(map[string]string)
	for _, l := range labels {
		parts := strings.SplitN(l, "=", 2)
		if len(parts) >= 2 {
			labelMap[parts[0]] = parts[1]
		}
	}

	files := r.MultipartForm.File["files"]
	if len(files) == 0 {
		writeError(w, http.StatusBadRequest, "at least one file is required")
		return
	}

	tempDir, err := os.MkdirTemp("", "oci-sync-push-*")
	if err != nil {
		writeError(w, http.StatusInternalServerError, "create temp dir")
		return
	}
	defer os.RemoveAll(tempDir)

	for _, fh := range files {
		src, err := fh.Open()
		if err != nil {
			writeError(w, http.StatusInternalServerError, fmt.Sprintf("open uploaded file: %v", err))
			return
		}

		// Preserve directory structure from webkitRelativePath
		relPath := fh.Filename
		destPath := filepath.Join(tempDir, filepath.FromSlash(relPath))

		if err := os.MkdirAll(filepath.Dir(destPath), 0o755); err != nil {
			src.Close()
			writeError(w, http.StatusInternalServerError, "create parent dir")
			return
		}

		dst, err := os.Create(destPath)
		if err != nil {
			src.Close()
			writeError(w, http.StatusInternalServerError, fmt.Sprintf("create file: %v", err))
			return
		}
		if _, err := io.Copy(dst, src); err != nil {
			src.Close()
			dst.Close()
			writeError(w, http.StatusInternalServerError, fmt.Sprintf("save file: %v", err))
			return
		}
		src.Close()
		dst.Close()
	}

	// Determine pack source: if single item, use it directly; otherwise use tempDir
	packPath := tempDir
	entries, err := os.ReadDir(tempDir)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "read temp dir")
		return
	}
	if len(entries) == 1 && entries[0].IsDir() {
		packPath = filepath.Join(tempDir, entries[0].Name())
	}

	data, err := archive.Pack(packPath)
	if err != nil {
		writeError(w, http.StatusInternalServerError, fmt.Sprintf("pack failed: %v", err))
		return
	}

	encrypted := passphrase != ""
	if encrypted {
		data, err = crypto.Encrypt(data, passphrase)
		if err != nil {
			writeError(w, http.StatusInternalServerError, fmt.Sprintf("encrypt failed: %v", err))
			return
		}
	}

	if err := oci.Push(r.Context(), data, ref, encrypted, labelMap); err != nil {
		writeError(w, http.StatusInternalServerError, fmt.Sprintf("push failed: %v", err))
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"success":   true,
		"ref":       ref,
		"size":      len(data),
		"encrypted": encrypted,
	})
}

func handlePull(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Repo       string `json:"repo"`
		Tag        string `json:"tag"`
		Passphrase string `json:"passphrase,omitempty"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}

	if req.Repo == "" || req.Tag == "" {
		writeError(w, http.StatusBadRequest, "repo and tag are required")
		return
	}

	ref := req.Repo + ":" + req.Tag
	ctx := r.Context()

	encrypted, err := oci.IsEncrypted(ctx, ref)
	if err != nil {
		writeError(w, http.StatusInternalServerError, fmt.Sprintf("check encryption: %v", err))
		return
	}

	if encrypted && req.Passphrase == "" {
		writeError(w, http.StatusBadRequest, "content is encrypted, passphrase is required")
		return
	}

	result, err := oci.Pull(ctx, ref)
	if err != nil {
		writeError(w, http.StatusInternalServerError, fmt.Sprintf("pull failed: %v", err))
		return
	}

	data := result.Data
	if result.Encrypted {
		data, err = crypto.Decrypt(data, req.Passphrase)
		if err != nil {
			writeError(w, http.StatusBadRequest, fmt.Sprintf("decrypt failed: %v", err))
			return
		}
	}

	w.Header().Set("Content-Type", "application/gzip")
	w.Header().Set("Content-Disposition", fmt.Sprintf(`attachment; filename="%s.tar.gz"`, req.Tag))
	w.WriteHeader(http.StatusOK)
	w.Write(data)
}

func handleDelete(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Repo string `json:"repo"`
		Tag  string `json:"tag"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body")
		return
	}

	if req.Repo == "" || req.Tag == "" {
		writeError(w, http.StatusBadRequest, "repo and tag are required")
		return
	}

	ref := req.Repo + ":" + req.Tag
	if err := oci.Delete(r.Context(), ref); err != nil {
		writeError(w, http.StatusInternalServerError, fmt.Sprintf("delete failed: %v", err))
		return
	}

	writeJSON(w, http.StatusOK, map[string]bool{"success": true})
}
