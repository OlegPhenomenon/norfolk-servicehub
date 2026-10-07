import type { ComponentProps, ReactNode } from 'react'
import { Link, type LinkProps } from 'react-router'
import { buttonClasses, type ButtonSize, type ButtonVariant } from './classes'
import { cn } from './cn'
import { Icon, type IconName } from './Icon'
import { Spinner } from './Spinner'

interface CommonProps {
  variant?: ButtonVariant
  size?: ButtonSize
  /** Icon before the label. */
  icon?: IconName
  /** Icon after the label. */
  iconRight?: IconName
  fullWidth?: boolean
  children?: ReactNode
}

export interface ButtonProps extends CommonProps, Omit<ComponentProps<'button'>, 'children'> {
  /** Shows a spinner, disables the button and sets `aria-busy`. */
  loading?: boolean
}

/**
 * <Button onClick={save}>Save</Button>
 * <Button variant="secondary" icon="plus">Add</Button>
 * <Button type="submit" loading={mutation.isPending}>Submit request</Button>
 * Default `type="button"` (pass `type="submit"` inside forms).
 */
export function Button({ variant = 'primary', size = 'md', icon, iconRight, fullWidth, loading, disabled, className, children, type = 'button', ...rest }: ButtonProps) {
  const iconSize = size === 'sm' ? 16 : 18
  return (
    <button type={type} disabled={disabled || loading} aria-busy={loading || undefined} className={cn(buttonClasses(variant, size, fullWidth), className)} {...rest}>
      {loading ? <Spinner size={iconSize} label={null} /> : icon ? <Icon name={icon} size={iconSize} /> : null}
      {children}
      {iconRight && !loading ? <Icon name={iconRight} size={iconSize} /> : null}
    </button>
  )
}

export interface ButtonLinkProps extends CommonProps, Omit<LinkProps, 'children'> {}

/** A router `<Link>` styled as a button: `<ButtonLink to="/services" icon="search">Find a service</ButtonLink>` */
export function ButtonLink({ variant = 'primary', size = 'md', icon, iconRight, fullWidth, className, children, ...rest }: ButtonLinkProps) {
  const iconSize = size === 'sm' ? 16 : 18
  return (
    <Link className={cn(buttonClasses(variant, size, fullWidth), className)} {...rest}>
      {icon ? <Icon name={icon} size={iconSize} /> : null}
      {children}
      {iconRight ? <Icon name={iconRight} size={iconSize} /> : null}
    </Link>
  )
}
