# `pbuilder` for Ruby

This branch specifies the deploy workflow for Debian packages of our custom Ruby builds.
To deploy:
- Commit your changes to any branch
- Tag your commit with a string of the form `v(digit).(digit).(digit)-(tagtext)`
- Push your commits and tags, and note the SHA of the commit
- Checkout the `debian_pbuilder` branch. `echo $SHA > build.version`, commit, and push
- Your commit on `debian_pbuilder` will now appear on the [Shipit stack](https://shipit.shopify.io/shopify/ruby/production).
Deploy it using the web UI - it will build, test, and upload `.deb`s to [PackageCloud](https://packages.shopify.io/).

For more context, read the [codelab](https://github.com/Shopify/codelabs/tree/master/building_a_new_ruby_version).
Also, see the [codelab](https://github.com/Shopify/codelabs/tree/master/deploying_debs_with_pbuilder_and_shipit) on pbuilder.
