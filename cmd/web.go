package cmd

import (
	"context"
	"errors"
	"fmt"
	"io/fs"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"charm.land/log/v2"
	"github.com/spf13/cobra"
	"github.com/tiramission/oci-sync/internal/web"
)

var webFS fs.FS

// SetWebFS sets the embedded frontend filesystem.
// Called from main.go before Execute().
func SetWebFS(f fs.FS) {
	webFS = f
}

func newWebCmd() *cobra.Command {
	var port int
	var dev bool

	cmd := &cobra.Command{
		Use:   "web",
		Short: "Start the web UI server",
		Long: `Start an HTTP server that serves the web UI and API.
The frontend is embedded in the binary (built with: cd web && bun run build).
Use --dev to enable CORS for local Vite dev server.`,
		RunE: func(cmd *cobra.Command, args []string) error {
			return runWeb(port, dev)
		},
	}

	cmd.Flags().IntVar(&port, "port", 8080, "HTTP server port")
	cmd.Flags().BoolVar(&dev, "dev", false, "Development mode (enable CORS, skip embedded frontend)")

	return cmd
}

func runWeb(port int, dev bool) error {
	srv := web.NewServer(port, dev, webFS)

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	go func() {
		<-ctx.Done()
		log.Info("Shutting down server...")
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		srv.Shutdown(shutdownCtx)
	}()

	if dev {
		log.Info("Starting oci-sync web server (dev mode)", "port", port)
	} else {
		log.Info("Starting oci-sync web server", "port", port)
	}
	fmt.Printf("Listening on http://localhost:%d\n", port)

	if err := srv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		return fmt.Errorf("server error: %w", err)
	}
	return nil
}
