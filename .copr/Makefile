.PHONY: srpm
srpm:
	VERSION=$$(grep '^version' Cargo.toml | head -n1 | sed -E 's/version = "(.*)"/\1/') ; \
	git archive --format=tar.gz --prefix=fp-appimage-updater-$$VERSION/ HEAD > fp-appimage-updater-$$VERSION.tar.gz ; \
	rpmbuild -bs --define "_sourcedir $$PWD" --define "_srcrpmdir $(outdir)" fp-appimage-updater.spec
