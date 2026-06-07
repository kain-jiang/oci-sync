package main

import (
	"embed"
	"io/fs"

	"github.com/tiramission/oci-sync/cmd"
)

//go:embed all:web/dist
var webDistFS embed.FS

func main() {
	// Extract the "web/dist" subtree from the embedded filesystem
	distFS, err := fs.Sub(webDistFS, "web/dist")
	if err != nil {
		// web/dist/ is empty or missing — run without embedded frontend
		distFS = nil
	}
	cmd.SetWebFS(distFS)
	cmd.Execute()
}
