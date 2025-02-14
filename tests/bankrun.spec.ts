
import { describe, it } from 'node:test';
import { BanksClient, ProgramTestContext, startAnchor } from 'solana-bankrun';
import { Lending } from '../target/types/lending';
import { Connection, Keypair, PublicKey } from '@solana/web3.js';
import { BankrunProvider } from 'anchor-bankrun';
import { PythSolanaReceiver } from '@pythnetwork/pyth-solana-receiver';
import { BankrunContextWrapper } from '../bankrun-utils/bankrunConnection';
import { BN, Program } from '@coral-xyz/anchor';
import { TOKEN_PROGRAM_ID } from '@solana/spl-token';
import { createAccount, createMint, mintTo } from 'spl-token-bankrun';

// @ts-ignore
import IDL from '../target/idl/lending.json';

describe("Lending smart contract test", async () => {
    let context: ProgramTestContext;
    let provider: BankrunProvider;
    let bankrunContextWrapper:  BankrunContextWrapper;
    let program: Program<Lending>;
    let banksClient: BanksClient;
    let signer: Keypair;
    let usdcBankAccount: PublicKey;
    let solBankAccount: PublicKey;

    // Pyth oracle program ID for price feeds
    const pyth = new PublicKey("7UVimffxr9ow1uXYxsr4LHAcV58mLzhmwaeKvJ1pjLiE");

    // Connect to Solana devnet
    const devnetConnection = new Connection("https://api.devnet.solana.com");

    // Get Pyth account info from devnet
    const accountInfo = await devnetConnection.getAccountInfo(pyth);

    // Initialize the test context with program and Pyth oracle
    context = await startAnchor(
        '', 
        [ { name: 'lending', programId: new PublicKey(IDL.address)}],
        [ {address: pyth, info: accountInfo}]
    );

    provider = new BankrunProvider(context);

    // SOL/USD price feed ID from Pyth
    const SOL_PRICE_FEED_ID = "0xeaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a";

    bankrunContextWrapper = new BankrunContextWrapper(context);

    const connection = bankrunContextWrapper.connection.toConnection();

    // Initialize Pyth price feed receiver
    const pythSolanaReceiver = new PythSolanaReceiver({
        connection,
        wallet: provider.wallet,
    });

     // Get SOL/USD price feed account
    const solUsdPriceFeedAccount = pythSolanaReceiver.getPriceFeedAccountAddress(0, SOL_PRICE_FEED_ID);
    const feedAccountInfo = await devnetConnection.getAccountInfo(solUsdPriceFeedAccount);

    // Set price feed account in test context
    context.setAccount(solUsdPriceFeedAccount, feedAccountInfo);

    console.log('pricefeed:', solUsdPriceFeedAccount);
    console.log('Pyth Account Info:', accountInfo);

     // Initialize program with IDL
    program = new Program<Lending>(IDL as Lending, provider);
    banksClient = context.banksClient;
    signer = provider.wallet.payer;

    // Create USDC mint
    const mintUSDC = await createMint(
        // @ts-ignore
        banksClient,
        signer,
        signer.publicKey,
        null,
        2
    );

    // Create SOL mint
    const mintSOL = await createMint(
        // @ts-ignore
        banksClient,
        signer,
        signer.publicKey,
        null,
        2
    );

    // Derive PDA for USDC bank account
    [usdcBankAccount] = PublicKey.findProgramAddressSync(
        [Buffer.from("treasury"), mintUSDC.toBuffer()],
        program.programId
    );

    console.log('USDC Bank Account', usdcBankAccount.toBase58());

    // Derive PDA for SOL bank account
    [solBankAccount] = PublicKey.findProgramAddressSync(
        [Buffer.from("treasury"), mintSOL.toBuffer()],
        program.programId
    );

    console.log('SOL Bank Account', solBankAccount.toBase58());

    // Test 1: Initialize user account
    it("Should init user", async () => {
        const initUserTx = await program.methods
        .initUser(mintUSDC)
        .accounts({
            signer: signer.publicKey,
        })
        .rpc({commitment: "confirmed"});
    
        console.log("Init User:", initUserTx);
    });


    // Test 2: Initialize USDC bank and fund it
    it("should init Bank and fund USDC", async () => {
        // Initialize USDC bank with interest rate parameters
        const initUSDCBankTx = await program.methods
        .initBank(new BN(1), new BN(1))
        .accounts({
            signer: signer.publicKey,
            mint: mintUSDC,
            tokenProgram: TOKEN_PROGRAM_ID, 
        })
        .rpc({commitment: "confirmed"});

        console.log("Create USDC Bank Account:", initUSDCBankTx);

        // Mint 10,000 USDC to bank account
        const amount = 10_000 * 10 ** 9;
        const mintTx = await mintTo(
            banksClient,
            signer,
            mintUSDC,
            usdcBankAccount,
            signer,
            amount
        );

        console.log("Mint USDC to Bank signature:", mintTx);

    });

    // Test 3: Initialize SOL bank and fund it
    it("Should init Bank and fund SOL", async () => {
        // Initialize SOL bank with interest rate parameters
        const initSOLBankTx = await program.methods
        .initBank(new BN(1), new BN(1))
        .accounts({
            signer: signer.publicKey,
            mint: mintSOL,
            tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc({commitment: "confirmed"});

        console.log("Create SOL Bank Account:", initSOLBankTx);

        // Mint 10,000 SOL to bank account
        const amount = 10_000 * 10 ** 9;
        const mintTx = await mintTo(
            // @ts-ignore
            banksClient,
            signer,
            mintSOL,
            solBankAccount,
            signer,
            amount
        )

        console.log("Mint SOL to Bank:", mintTx);
    });

    // Test 4: Create and fund user token accounts
    it("should create and fund Token Accounts", async () => {
        const USDCTokenAccount = await createAccount(
            //@ts-ignore
            banksClient,
            signer,
            mintUSDC,
            signer.publicKey
        );

        console.log("Create USDC Token Account:", USDCTokenAccount);

        // Mint 10,000 USDC to user's token account
        const amount = 10_000 * 10 ** 9;
        const mintUSDCTx = await mintTo(
            // @ts-ignore
            banksClient,
            signer,
            mintUSDC,
            USDCTokenAccount,
            signer,
            amount
        )

        console.log("Mint USDC to Token Account:", mintUSDCTx);
    });

    // Test 5: Test deposit functionality
    it("Test deposit", async () => {
        const depositUSDCTx = await program.methods
        .deposit(new BN(100000000000))
        .accounts({
            signer: signer.publicKey,
            mint: mintUSDC,
            tokenProgram: TOKEN_PROGRAM_ID
        })
        .rpc({commitment: "confirmed"});

        console.log("Deposit USDC to vault:", depositUSDCTx);
    });

    // Test 6: Test borrow functionality
    it("Test borrow", async () => {
        // Borrow 1 SOL using USDC as collateral
        const borrowSOLTx = await program.methods
        .borrow(new BN(1))
        .accounts({
            signer: signer.publicKey,
            mint: mintSOL,
            tokenProgram: TOKEN_PROGRAM_ID,
            priceUpdate: solUsdPriceFeedAccount
        })
        .rpc({commitment: "confirmed"});

        console.log("Borrow SOL from vault:", borrowSOLTx);
    });

    // Test 7: Test repay functionality
    it("Test repay", async () => {
        // Repay 1 SOL to the lending vault
        const repaySOLTx = await program.methods
        .repay(new BN(1))
        .accounts({
            signer: signer.publicKey,
            mint: mintSOL,
            tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc({commitment: "confirmed"});

        console.log("Repay SOL to the vault:", repaySOLTx);
    });

    // Test 8: Test withdraw functionality
    it("Test withdraw", async () => {
        // Withdraw 100 USDC from lending vault
        const withdrawUSDCTx = await program.methods
        .withdraw(new BN(100))
        .accounts({
            signer: signer.publicKey,
            mint: mintUSDC,
            tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc({commitment: "confirmed"});

        console.log("Withdraw USDC:", withdrawUSDCTx);
    });

});

       


