// Minimal Go client for `openrage-cli serve`.
//
// It spawns the server as a child process and talks NDJSON over the
// child's stdin/stdout: one JSON request per line, one JSON response
// per line. Child stderr is forwarded to our stderr for logs.
//
// Build the server first:
//
//	cargo build --release --bin openrage-cli
//
// Then run, pointing at the built binary:
//
//	go run openrage_client.go /path/to/target/release/openrage-cli
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
)

// Client wraps a spawned `openrage-cli serve` process.
type Client struct {
	cmd *exec.Cmd
	in  io.WriteCloser
	out *bufio.Reader
	id  int
}

// NewClient spawns `<bin> serve` and wires up the pipes.
func NewClient(bin string, args ...string) (*Client, error) {
	cmd := exec.Command(bin, append([]string{"serve"}, args...)...)
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	cmd.Stderr = os.Stderr // server logs
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	return &Client{cmd: cmd, in: stdin, out: bufio.NewReader(stdout)}, nil
}

// Call sends one request and returns the decoded response object.
func (c *Client) Call(cmd string, params map[string]any) (map[string]any, error) {
	c.id++
	req := map[string]any{"id": c.id, "cmd": cmd}
	for k, v := range params {
		req[k] = v
	}
	line, err := json.Marshal(req)
	if err != nil {
		return nil, err
	}
	if _, err := c.in.Write(append(line, '\n')); err != nil {
		return nil, err
	}
	respLine, err := c.out.ReadBytes('\n')
	if err != nil {
		return nil, err
	}
	var resp map[string]any
	if err := json.Unmarshal(respLine, &resp); err != nil {
		return nil, err
	}
	if ok, _ := resp["ok"].(bool); !ok {
		return resp, fmt.Errorf("server error: %v", resp["error"])
	}
	return resp, nil
}

// Close asks the server to shut down and waits for it to exit.
func (c *Client) Close() error {
	_, _ = c.Call("shutdown", nil)
	_ = c.in.Close()
	return c.cmd.Wait()
}

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: openrage_client <path-to-openrage-cli>")
		os.Exit(2)
	}
	client, err := NewClient(os.Args[1])
	if err != nil {
		panic(err)
	}
	defer client.Close()

	if _, err := client.Call("ping", nil); err != nil {
		panic(err)
	}

	// Mount the app's selected game up to build 2699, all DLC.
	mounted, err := client.Call("mount", map[string]any{"dlc": "2699"})
	if err != nil {
		panic(err)
	}
	fmt.Printf("mounted %v packs from %v\n", mounted["packs"], mounted["gameRoot"])

	// Search the weapon registry.
	res, err := client.Call("search", map[string]any{
		"fileTypes": []string{"WEAPONINFO_FILE"},
		"limit":     5,
	})
	if err != nil {
		panic(err)
	}
	fmt.Printf("weapon files: %v (showing up to 5)\n", res["count"])
	for _, h := range res["hits"].([]any) {
		hit := h.(map[string]any)
		fmt.Printf("  %s  <=  %s\n", hit["path"], hit["container"])
	}
}
