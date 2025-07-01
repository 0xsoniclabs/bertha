package main

import (
	"log"
	"os"

	"github.com/0xsoniclabs/blockdb/cmd/block-db/app"
)

func main() {
	if err := app.Run(os.Args); err != nil {
		log.Fatal(err)
	}
}
