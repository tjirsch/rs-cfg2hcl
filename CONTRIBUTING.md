# Contributing to cfg2hcl

Thank you for your interest in contributing to cfg2hcl!
Contributions from the community to help improve this project are welcome.

## How to Contribute

### Reporting Bugs

If you find a bug, please create a new issue using the [Bug Report template](.github/ISSUE_TEMPLATE/bug_report.md). Be sure to include:
- A clear description of the issue
- Steps to reproduce
- Expected vs. actual behavior
- Any relevant logs or screenshots

### Suggesting Enhancements

If you have an idea for a new feature or improvement, please create a new issue using the [Feature Request template](.github/ISSUE_TEMPLATE/feature_request.md).

### Pull Requests

1.  **Fork the repository** and create your branch from `main`.
2.  **Clone the repository** to your local machine.
3.  **Create a new branch** for your feature or bug fix:
    ```bash
    git checkout -b feature/my-new-feature
    ```
4.  **Make your changes**. Ensure your code follows the project's coding standards.
5.  **Test your changes**. Run existing tests and add new ones if necessary.
6.  **Commit your changes** with descriptive commit messages.
7.  **Push your branch** to your fork:
    ```bash
    git push origin feature/my-new-feature
    ```
8.  **Open a Pull Request** against the `main` branch of the original repository.

## Development Setup

1.  Ensure you have Rust installed (latest stable version recommended).
2.  Clone the repository.
3.  Run `cargo build` to verify the build.
4.  Run `cargo test` to run the test suite.

## Coding Standards

-   Follow standard Rust idioms and best practices.
-   Use `cargo fmt` to format your code before committing.
-   Use `cargo clippy` to catch common mistakes and improve code quality.

## License

By contributing to this project, you agree that your contributions will be licensed under the MIT License.
