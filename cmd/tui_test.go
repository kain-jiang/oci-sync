package cmd

import (
	"testing"
)

func TestNewTuiCmd(t *testing.T) {
	cmd := newTuiCmd()
	if cmd.Use != "tui" {
		t.Errorf("expected command use to be 'tui', got %q", cmd.Use)
	}
	if cmd.Short == "" {
		t.Error("expected short description to be set")
	}
	if cmd.RunE == nil {
		t.Error("expected RunE function to be set")
	}
}
