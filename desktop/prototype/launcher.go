package main

import (
	"errors"
	"flag"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"time"
)

func main() {
	noBrowser := flag.Bool("no-browser", false, "Serve the prototype without opening a browser.")
	flag.Parse()

	executable, err := os.Executable()
	if err != nil {
		log.Fatal(err)
	}
	site := filepath.Join(filepath.Dir(executable), "site")
	if _, err := os.Stat(filepath.Join(site, "prototype", "index.html")); err != nil {
		log.Fatal("Prototype files are missing. Extract the complete ZIP before opening the launcher.")
	}

	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		log.Fatal(err)
	}
	host := listener.Addr().String()
	url := "http://" + host + "/prototype/"
	files := http.FileServer(http.Dir(site))
	server := &http.Server{
		ReadHeaderTimeout: 5 * time.Second,
		IdleTimeout:       30 * time.Second,
		Handler: http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if r.Host != host {
				http.Error(w, "Invalid host", http.StatusBadRequest)
				return
			}
			if r.Method != http.MethodGet && r.Method != http.MethodHead {
				w.Header().Set("Allow", "GET, HEAD")
				http.Error(w, "Read-only prototype", http.StatusMethodNotAllowed)
				return
			}
			w.Header().Set("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; worker-src 'self' blob:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'")
			w.Header().Set("X-Content-Type-Options", "nosniff")
			w.Header().Set("Cache-Control", "no-store")
			files.ServeHTTP(w, r)
		}),
	}

	fmt.Printf("Velella UI Prototype\n%s\nKeep this window open during preview. Close it to stop.\n", url)
	if !*noBrowser {
		go func() {
			if err := exec.Command("rundll32.exe", "url.dll,FileProtocolHandler", url).Run(); err != nil {
				log.Printf("Open the URL above in Edge or Chrome: %v", err)
			}
		}()
	}
	if err := server.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
		log.Fatal(err)
	}
}
