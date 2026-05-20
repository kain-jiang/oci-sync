package cmd

import (
	"context"
	"fmt"
	"strings"

	"charm.land/log/v2"
	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"github.com/spf13/cobra"
	"github.com/tiramission/oci-sync/internal/config"
	"github.com/tiramission/oci-sync/internal/oci"
)

type tuiState int

const (
	stateShortcuts tuiState = iota
	stateArtifactsLoading
	stateArtifacts
	stateInputPath
	stateInputPassphrase
	stateConfirmDelete
	stateStatus
)

// Messages
type fetchArtifactsMsg struct {
	artifacts []oci.ArtifactInfo
	err       error
}

type pullResultMsg struct {
	err error
}

type deleteResultMsg struct {
	err error
}

type model struct {
	state            tuiState
	shortcuts        []config.ShortcutInfo
	shortcutIndex    int
	selectedShortcut config.ShortcutInfo

	artifacts        []oci.ArtifactInfo
	artifactIndex    int
	selectedArtifact oci.ArtifactInfo

	// Input controls
	textInput        textinput.Model
	targetPath       string
	passphrase       string

	// Status messages
	statusMsg        string
	statusHeader     string
	err              error
	lastAction       string // "pull" or "delete"

	// Deletion confirm
	confirmDeleteIndex int // 0 = No, 1 = Yes

	// Terminal size
	terminalWidth  int
	terminalHeight int
}

func newTuiCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "tui",
		Short: "Interactive TUI to manage shortcut artifacts",
		Long:  `Launch a full-screen interactive terminal UI to view shortcuts, list remote tags, and pull or delete artifacts.`,
		RunE: func(cmd *cobra.Command, args []string) error {
			return runTui(cmd.Context())
		},
	}
	return cmd
}

func runTui(ctx context.Context) error {
	// Configure logging level to avoid corrupting terminal stdout
	origLevel := log.GetLevel()
	log.SetLevel(log.ErrorLevel)
	defer log.SetLevel(origLevel)

	shortcuts := config.GetAllShortcuts()
	if len(shortcuts) == 0 {
		log.SetLevel(origLevel)
		fmt.Println("No shortcuts configured. Use 'oci-sync alias add <name> --repo <repo>' to add one first.")
		return nil
	}

	ti := textinput.New()
	ti.Placeholder = "Enter value..."
	ti.Focus()

	m := model{
		state:         stateShortcuts,
		shortcuts:     shortcuts,
		shortcutIndex: 0,
		textInput:     ti,
	}

	p := tea.NewProgram(m, tea.WithAltScreen())
	_, err := p.Run()
	return err
}

func (m model) Init() tea.Cmd {
	return textinput.Blink
}

func fetchArtifactsCmd(repo string) tea.Cmd {
	return func() tea.Msg {
		artifacts, err := oci.List(context.Background(), repo)
		return fetchArtifactsMsg{artifacts: artifacts, err: err}
	}
}

func pullArtifactCmd(shortcutName, tag, localPath, passphrase string) tea.Cmd {
	return func() tea.Msg {
		remotePath, err := buildShortcutRemoteRef(shortcutName, tag)
		if err != nil {
			return pullResultMsg{err: err}
		}
		err = runPull(context.Background(), remotePath, localPath, passphrase)
		return pullResultMsg{err: err}
	}
}

func deleteArtifactCmd(shortcutName, tag string) tea.Cmd {
	return func() tea.Msg {
		remotePath, err := buildShortcutRemoteRef(shortcutName, tag)
		if err != nil {
			return deleteResultMsg{err: err}
		}
		err = runDelete(context.Background(), remotePath)
		return deleteResultMsg{err: err}
	}
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmd tea.Cmd

	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.terminalWidth = msg.Width
		m.terminalHeight = msg.Height
		return m, nil

	case tea.KeyMsg:
		switch msg.String() {
		case "ctrl+c":
			return m, tea.Quit
		}

		// Handle keys based on state
		switch m.state {
		case stateShortcuts:
			switch msg.String() {
			case "up", "j":
				m.shortcutIndex = (m.shortcutIndex - 1 + len(m.shortcuts)) % len(m.shortcuts)
			case "down", "k":
				m.shortcutIndex = (m.shortcutIndex + 1) % len(m.shortcuts)
			case "enter", "right", "l":
				m.selectedShortcut = m.shortcuts[m.shortcutIndex]
				m.state = stateArtifactsLoading
				return m, fetchArtifactsCmd(m.selectedShortcut.Repo)
			case "tab":
				// If we already have fetched artifacts for this shortcut, we can focus it directly
				if len(m.artifacts) > 0 {
					m.state = stateArtifacts
				}
			case "q":
				return m, tea.Quit
			}

		case stateArtifactsLoading:
			switch msg.String() {
			case "esc", "q":
				m.state = stateShortcuts
			}

		case stateArtifacts:
			numOptions := len(m.artifacts)
			switch msg.String() {
			case "up", "j":
				if numOptions > 0 {
					m.artifactIndex = (m.artifactIndex - 1 + numOptions) % numOptions
				}
			case "down", "k":
				if numOptions > 0 {
					m.artifactIndex = (m.artifactIndex + 1) % numOptions
				}
			case "tab", "left", "h", "esc":
				m.state = stateShortcuts
			case "p":
				if numOptions > 0 {
					m.selectedArtifact = m.artifacts[m.artifactIndex]
					m.state = stateInputPath
					m.textInput.SetValue("./" + m.selectedArtifact.Tag)
					m.textInput.Focus()
					m.textInput.EchoMode = textinput.EchoNormal
				}
			case "d":
				if numOptions > 0 {
					m.selectedArtifact = m.artifacts[m.artifactIndex]
					m.state = stateConfirmDelete
					m.confirmDeleteIndex = 0
				}
			case "r":
				m.state = stateArtifactsLoading
				return m, fetchArtifactsCmd(m.selectedShortcut.Repo)
			case "q":
				return m, tea.Quit
			}

		case stateInputPath:
			switch msg.String() {
			case "enter":
				path := strings.TrimSpace(m.textInput.Value())
				if path != "" {
					m.targetPath = path
					if m.selectedArtifact.Encrypted {
						m.state = stateInputPassphrase
						m.textInput.SetValue("")
						m.textInput.EchoMode = textinput.EchoPassword
						m.textInput.EchoCharacter = '*'
						m.textInput.Focus()
					} else {
						m.state = stateStatus
						m.statusHeader = "Pulling Artifact"
						m.statusMsg = fmt.Sprintf("Pulling %s:%s to %s...", m.selectedShortcut.Name, m.selectedArtifact.Tag, m.targetPath)
						m.err = nil
						m.lastAction = "pull"
						return m, pullArtifactCmd(m.selectedShortcut.Name, m.selectedArtifact.Tag, m.targetPath, "")
					}
				}
			case "esc":
				m.state = stateArtifacts
			default:
				m.textInput, cmd = m.textInput.Update(msg)
				return m, cmd
			}

		case stateInputPassphrase:
			switch msg.String() {
			case "enter":
				m.passphrase = m.textInput.Value()
				m.state = stateStatus
				m.statusHeader = "Pulling Artifact"
				m.statusMsg = fmt.Sprintf("Pulling and decrypting %s:%s to %s...", m.selectedShortcut.Name, m.selectedArtifact.Tag, m.targetPath)
				m.err = nil
				m.lastAction = "pull"
				return m, pullArtifactCmd(m.selectedShortcut.Name, m.selectedArtifact.Tag, m.targetPath, m.passphrase)
			case "esc":
				m.state = stateInputPath
				m.textInput.SetValue(m.targetPath)
				m.textInput.EchoMode = textinput.EchoNormal
				m.textInput.Focus()
			default:
				m.textInput, cmd = m.textInput.Update(msg)
				return m, cmd
			}

		case stateConfirmDelete:
			switch msg.String() {
			case "left", "h":
				m.confirmDeleteIndex = 0
			case "right", "l":
				m.confirmDeleteIndex = 1
			case "enter":
				if m.confirmDeleteIndex == 1 {
					m.state = stateStatus
					m.statusHeader = "Deleting Artifact"
					m.statusMsg = fmt.Sprintf("Deleting %s:%s from registry...", m.selectedShortcut.Name, m.selectedArtifact.Tag)
					m.err = nil
					m.lastAction = "delete"
					return m, deleteArtifactCmd(m.selectedShortcut.Name, m.selectedArtifact.Tag)
				} else {
					m.state = stateArtifacts
				}
			case "esc":
				m.state = stateArtifacts
			case "q":
				return m, tea.Quit
			}

		case stateStatus:
			if m.err != nil || m.statusHeader == "Success" {
				switch msg.String() {
				case "enter", "esc":
					if m.lastAction == "delete" && m.err == nil {
						m.state = stateArtifactsLoading
						return m, fetchArtifactsCmd(m.selectedShortcut.Repo)
					}
					m.state = stateArtifacts
				}
			}
		}

	case fetchArtifactsMsg:
		if msg.err != nil {
			m.state = stateStatus
			m.statusHeader = "Error Listing Artifacts"
			m.err = msg.err
		} else {
			m.artifacts = msg.artifacts
			m.artifactIndex = 0
			m.state = stateArtifacts
		}

	case pullResultMsg:
		if msg.err != nil {
			m.state = stateStatus
			m.statusHeader = "Pull Failed"
			m.err = msg.err
		} else {
			m.state = stateStatus
			m.statusHeader = "Success"
			m.statusMsg = fmt.Sprintf("Successfully pulled and unpacked artifact to: %s", m.targetPath)
		}

	case deleteResultMsg:
		if msg.err != nil {
			m.state = stateStatus
			m.statusHeader = "Delete Failed"
			m.err = msg.err
		} else {
			m.state = stateStatus
			m.statusHeader = "Success"
			m.statusMsg = fmt.Sprintf("Successfully deleted artifact %s:%s", m.selectedShortcut.Name, m.selectedArtifact.Tag)
		}
	}

	return m, nil
}

// Lip Gloss styles
var (
	styleTitle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#BD93F9")).
			Border(lipgloss.NormalBorder(), false, false, true, false).
			BorderForeground(lipgloss.Color("#FF79C6")).
			Padding(0, 1).
			MarginBottom(1)

	styleSelected = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#00FFFF"))

	styleNormal = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#F8F8F2"))

	styleHeading = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#FF79C6")).
			MarginBottom(1)

	styleHelp = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#6272A4")).
			Border(lipgloss.NormalBorder(), true, false, false, false).
			BorderForeground(lipgloss.Color("#44475A")).
			PaddingTop(1)

	styleError = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#FF5555"))

	styleSuccess = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#50FA7B"))

	styleBox = lipgloss.NewStyle().
			Border(lipgloss.RoundedBorder()).
			BorderForeground(lipgloss.Color("#BD93F9")).
			Padding(1, 2)
)

func (m model) getDimensions() (sidebarW, mainW, panesH, detailsH int) {
	w, h := m.terminalWidth, m.terminalHeight
	if w <= 0 {
		w = 100
	}
	if h <= 0 {
		h = 30
	}

	sidebarW = 30
	if w < 90 {
		sidebarW = 24
	}

	mainW = w - sidebarW - 6
	if mainW < 20 {
		mainW = 20
	}

	detailsH = 6
	if h > 35 {
		detailsH = 8
	}

	panesH = h - detailsH - 9
	if panesH < 8 {
		panesH = 8
	}

	return
}

func truncate(s string, l int) string {
	if len(s) <= l {
		return s
	}
	if l > 3 {
		return s[:l-3] + "..."
	}
	return s[:l]
}

func (m model) View() string {
	sidebarW, mainW, panesH, detailsH := m.getDimensions()
	header := styleTitle.Render("⚡ OCI-SYNC TUI ⚡")

	// 1. Sidebar View (Shortcuts)
	sidebarStyle := lipgloss.NewStyle().
		Border(lipgloss.RoundedBorder()).
		BorderForeground(lipgloss.Color("#44475A")).
		Width(sidebarW).
		Height(panesH)
	if m.state == stateShortcuts {
		sidebarStyle = sidebarStyle.BorderForeground(lipgloss.Color("#00FFFF"))
	}

	var sb strings.Builder
	sb.WriteString(styleHeading.Render(" Shortcuts") + "\n")
	for i, s := range m.shortcuts {
		shortcutName := truncate(s.Name, sidebarW-6)
		if i == m.shortcutIndex && m.state == stateShortcuts {
			sb.WriteString(styleSelected.Render(fmt.Sprintf(" ▸ %s", shortcutName)) + "\n")
		} else if i == m.shortcutIndex {
			sb.WriteString(styleNormal.Render(fmt.Sprintf(" • %s", shortcutName)) + "\n")
		} else {
			sb.WriteString(styleNormal.Render(fmt.Sprintf("   %s", shortcutName)) + "\n")
		}
	}
	sidebarView := sidebarStyle.Render(sb.String())

	// 2. Main View (Artifacts)
	mainStyle := lipgloss.NewStyle().
		Border(lipgloss.RoundedBorder()).
		BorderForeground(lipgloss.Color("#44475A")).
		Width(mainW).
		Height(panesH)
	if m.state == stateArtifacts {
		mainStyle = mainStyle.BorderForeground(lipgloss.Color("#00FFFF"))
	}

	var mb strings.Builder
	repoTitle := "Artifacts"
	if m.selectedShortcut.Repo != "" {
		repoTitle = fmt.Sprintf("Artifacts (%s)", truncate(m.selectedShortcut.Repo, mainW-15))
	}
	mb.WriteString(styleHeading.Render(" "+repoTitle) + "\n")

	switch m.state {
	case stateShortcuts:
		mb.WriteString(styleNormal.Render("\n  Select a shortcut on the left\n  and press Enter to fetch tags."))
	case stateArtifactsLoading:
		mb.WriteString(styleNormal.Render("\n  Loading repository tags from registry...\n  Please wait."))
	case stateArtifacts:
		if len(m.artifacts) == 0 {
			mb.WriteString(styleNormal.Render("\n  No artifacts found in this repository."))
		} else {
			// Header
			mb.WriteString(styleHeading.Render(fmt.Sprintf("  %-20s %-10s %-10s %-10s", "TAG", "SIZE", "ENCRYPTED", "VERSION")) + "\n")
			for i, a := range m.artifacts {
				tagStr := truncate(a.Tag, 18)
				sizeStr := formatBytes(int(a.Size))
				encStr := "No"
				if a.Encrypted {
					encStr = "Yes"
				}
				verStr := truncate(a.Version, 8)
				rowStr := fmt.Sprintf("  %-20s %-10s %-10s %-10s", tagStr, sizeStr, encStr, verStr)

				if i == m.artifactIndex {
					mb.WriteString(styleSelected.Render(" ▸"+rowStr[2:]))
				} else {
					mb.WriteString(styleNormal.Render(rowStr))
				}
				mb.WriteString("\n")
			}
		}
	}
	mainView := mainStyle.Render(mb.String())

	// Combine top row
	topRow := lipgloss.JoinHorizontal(lipgloss.Top, sidebarView, mainView)

	// 3. Details View (Bottom)
	detailsStyle := lipgloss.NewStyle().
		Border(lipgloss.RoundedBorder()).
		BorderForeground(lipgloss.Color("#44475A")).
		Width(sidebarW + mainW + 3).
		Height(detailsH)

	var db strings.Builder
	db.WriteString(styleHeading.Render(" Details & Status") + "\n")
	if len(m.artifacts) > 0 && m.artifactIndex < len(m.artifacts) && (m.state == stateArtifacts) {
		a := m.artifacts[m.artifactIndex]
		db.WriteString(fmt.Sprintf("  Full Name : %s\n", truncate(a.FullName, sidebarW+mainW-16)))
		db.WriteString(fmt.Sprintf("  Digest    : %s\n", a.Digest))
		db.WriteString(fmt.Sprintf("  Version   : %s | Size: %s | Encrypted: %t\n", a.Version, formatBytes(int(a.Size)), a.Encrypted))
		if len(a.Labels) > 0 {
			var lps []string
			for k, v := range a.Labels {
				lps = append(lps, fmt.Sprintf("%s=%s", k, v))
			}
			db.WriteString(fmt.Sprintf("  Labels    : %s\n", truncate(strings.Join(lps, ", "), sidebarW+mainW-16)))
		}
	} else {
		db.WriteString("  No artifact selected.")
	}
	detailsView := detailsStyle.Render(db.String())

	// 4. Help View (Footer)
	helpWidth := sidebarW + mainW + 3
	var helpText string
	switch m.state {
	case stateShortcuts:
		helpText = " ↑/↓: navigate shortcuts • enter/→: load tags • tab: focus tags • q: quit"
	case stateArtifactsLoading:
		helpText = " esc: cancel load • q: quit"
	case stateArtifacts:
		helpText = " ↑/↓: navigate tags • p: pull • d: delete • r: refresh • esc/tab/←: focus shortcuts • q: quit"
	}
	helpView := styleHelp.Width(helpWidth).Render(helpText)

	// Assemble page
	page := lipgloss.JoinVertical(lipgloss.Left, header, topRow, detailsView, helpView)

	// 5. Draw modal overlay if a dialog is active
	if m.state == stateInputPath || m.state == stateInputPassphrase || m.state == stateConfirmDelete || m.state == stateStatus {
		var dialogContent string
		dialogWidth := 50
		dialogHeight := 8

		switch m.state {
		case stateInputPath:
			dialogContent = fmt.Sprintf(
				"%s\n\n%s\n\n[ enter: submit • esc: cancel ]",
				styleHeading.Render("Local Destination Directory:"),
				m.textInput.View(),
			)
		case stateInputPassphrase:
			dialogContent = fmt.Sprintf(
				"%s\n\n%s\n\n[ enter: submit • esc: cancel ]",
				styleHeading.Render("Decryption Passphrase:"),
				m.textInput.View(),
			)
		case stateConfirmDelete:
			noStr := "  No  "
			yesStr := "  Yes  "
			if m.confirmDeleteIndex == 0 {
				noStr = styleSelected.Render("[ No ]")
				yesStr = styleNormal.Render(" Yes ")
			} else {
				noStr = styleNormal.Render(" No ")
				yesStr = styleSelected.Render("[ Yes ]")
			}
			dialogContent = fmt.Sprintf(
				"%s\n%s\n\n%s    %s\n\n[ ←/→: select • enter: confirm • esc: cancel ]",
				styleHeading.Render("Delete Remote Artifact?"),
				styleError.Render(fmt.Sprintf("Target: %s:%s", m.selectedShortcut.Name, m.selectedArtifact.Tag)),
				noStr,
				yesStr,
			)
		case stateStatus:
			var sb strings.Builder
			if m.err != nil {
				sb.WriteString(styleError.Render(m.statusHeader) + "\n\n")
				sb.WriteString(truncate(m.err.Error(), dialogWidth-6) + "\n\n")
				sb.WriteString("[ enter: continue ]")
			} else if m.statusHeader == "Success" {
				sb.WriteString(styleSuccess.Render("Success ✓") + "\n\n")
				sb.WriteString(truncate(m.statusMsg, dialogWidth-6) + "\n\n")
				sb.WriteString("[ enter: continue ]")
			} else {
				sb.WriteString(styleHeading.Render(m.statusHeader) + "\n\n")
				sb.WriteString(m.statusMsg + "\n")
			}
			dialogContent = sb.String()
		}

		dialogBox := lipgloss.NewStyle().
			Border(lipgloss.DoubleBorder()).
			BorderForeground(lipgloss.Color("#BD93F9")).
			Width(dialogWidth).
			Height(dialogHeight).
			Padding(1, 2).
			Render(dialogContent)

		w := m.terminalWidth
		h := m.terminalHeight
		if w <= 0 {
			w = 100
		}
		if h <= 0 {
			h = 30
		}
		return lipgloss.Place(w, h, lipgloss.Center, lipgloss.Center, dialogBox)
	}

	return page
}
